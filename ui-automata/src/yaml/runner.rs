use std::collections::HashMap;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::{
    AnchorDef, AutomataError, Desktop, Executor, LaunchContext, LaunchWait, Plan, RecoveryHandler,
    executor::WorkflowState, platform::Element,
};

use super::{PhaseEvent, WorkflowFile, YamlPhase};

use crate::action::sub_output as sub_action_output;

/// Resolve a subflow path:
/// 1. If absolute, use as-is.
/// 2. If the parent workflow has a source path, try relative to its directory.
/// 3. Fall back to `~/.ui-automata/workflows/<subflow>`.
fn resolve_subflow_path(
    subflow: &str,
    source_path: Option<&std::path::Path>,
) -> std::path::PathBuf {
    let sf = std::path::Path::new(subflow);
    if sf.is_absolute() {
        return sf.to_path_buf();
    }
    if let Some(src) = source_path {
        if let Some(parent) = src.parent() {
            let candidate = parent.join(subflow);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // Fall back to ~/.ui-automata/workflows/
    if let Some(home) = dirs::home_dir() {
        home.join(".ui-automata").join("workflows").join(subflow)
    } else {
        std::path::PathBuf::from(subflow)
    }
}

impl WorkflowFile {
    /// Execute the workflow against a live desktop.
    ///
    /// If `on_event` is `Some`, the callback is invoked synchronously for each
    /// phase-level progress event as each phase starts, completes, or fails, and
    /// a final `Completed` or `Failed` event is fired when the workflow finishes.
    ///
    /// If `cancel` is `Some`, it is checked between phases. When the flag is set the
    /// run returns `AutomataError::Cancelled` immediately (after firing a `Failed`
    /// progress event).
    ///
    /// Returns the `WorkflowState` (vars + output) on success.
    pub fn run<D: Desktop>(
        self,
        executor: &mut Executor<D>,
        on_event: Option<&mut dyn FnMut(PhaseEvent)>,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<WorkflowState, AutomataError> {
        self.run_inner(executor, on_event, cancel, 0)
    }

    fn run_inner<D: Desktop>(
        mut self,
        executor: &mut Executor<D>,
        mut on_event: Option<&mut dyn FnMut(PhaseEvent)>,
        cancel: Option<&std::sync::atomic::AtomicBool>,
        depth: usize,
    ) -> Result<WorkflowState, AutomataError> {
        macro_rules! send {
            ($e:expr) => {
                if let Some(f) = on_event.as_mut() {
                    (**f)($e);
                }
            };
        }

        // Set the depth on the DOM for anchor scoping.
        executor.dom.set_depth(depth);

        let mut state = WorkflowState::new(self.defaults.action_snapshot);
        state.params = self.params_resolved.clone();

        let default_timeout = self
            .defaults
            .timeout
            .unwrap_or(crate::plan::DEFAULT_TIMEOUT);
        let default_retry = self.defaults.retry;
        // Extract source_path before we partially move self.phases.
        let source_path = self.source_path.clone();

        // Launch app if declared; wait for the window using the configured strategy.
        // Only performed at the top-level workflow (depth == 0).
        if depth == 0 {
            if let Some(launch) = &self.launch {
                // Snapshot existing HWNDs before launch (used by new_window and wait_for).
                let pre_hwnds: HashSet<u64> =
                    if launch.wait == LaunchWait::NewWindow || launch.wait_for.is_some() {
                        executor
                            .desktop
                            .application_windows()
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|w| w.hwnd())
                            .collect()
                    } else {
                        HashSet::new()
                    };

                // Resolve what to launch: `app` takes precedence over `exe`.
                let target = match (&launch.app, &launch.exe) {
                    (Some(a), _) => a.clone(),
                    (None, Some(e)) => e.clone(),
                    (None, None) => {
                        return Err(AutomataError::Internal(
                            "launch: requires either `exe` or `app`".into(),
                        ));
                    }
                };

                let pid = executor.desktop.open_application(&target).map_err(|e| {
                    AutomataError::Internal(format!("failed to launch '{target}': {e}"))
                })?;
                log::info!("launched '{target}' pid={pid}");

                // Derive process name from the target (strip path and .exe suffix).
                let process_name = target
                    .trim_end_matches(".exe")
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&target)
                    .to_lowercase();

                // For wait_for, resolve the anchor's process name and poll for a new window.
                if let Some(anchor_name) = &launch.wait_for {
                    let anchor_process = self
                        .anchors
                        .get(anchor_name)
                        .and_then(|a| a.process.clone())
                        .map(|p| p.to_lowercase())
                        .unwrap_or_else(|| process_name.clone());
                    let timeout = launch
                        .timeout
                        .or(self.defaults.timeout)
                        .unwrap_or(Duration::from_secs(15));
                    let deadline = Instant::now() + timeout;
                    log::info!(
                        "launch: waiting for '{anchor_name}' window (process={anchor_process}, timeout={timeout:?})"
                    );
                    loop {
                        let windows = executor.desktop.application_windows().unwrap_or_default();
                        let found = windows.iter().any(|w| {
                            w.process_name()
                                .map(|n| n.to_lowercase() == anchor_process)
                                .unwrap_or(false)
                                && w.hwnd().map_or(false, |h| !pre_hwnds.contains(&h))
                        });
                        if found {
                            break;
                        }
                        if Instant::now() >= deadline {
                            executor.cleanup_depth(depth);
                            executor.dom.set_depth(depth.saturating_sub(1));
                            return Err(AutomataError::Internal(format!(
                                "timed out waiting for '{anchor_name}' window (process={anchor_process})"
                            )));
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    executor.dom.set_launch_context(LaunchContext {
                        wait: LaunchWait::NewWindow,
                        pid,
                        pre_hwnds: pre_hwnds.clone(),
                        process_name: anchor_process,
                    });
                }

                // For new_pid and new_window, poll until the expected window appears.
                if launch.wait != LaunchWait::MatchAny && launch.wait_for.is_none() {
                    let timeout = launch
                        .timeout
                        .or(self.defaults.timeout)
                        .unwrap_or(Duration::from_secs(15));
                    let deadline = Instant::now() + timeout;
                    log::info!(
                        "launch: waiting for {target} window (strategy={:?}, timeout={timeout:?})",
                        launch.wait
                    );
                    loop {
                        let windows = executor.desktop.application_windows().unwrap_or_default();
                        let found = match launch.wait {
                            LaunchWait::NewPid => windows
                                .iter()
                                .any(|w| w.process_id().map_or(false, |p| p == pid)),
                            LaunchWait::NewWindow => windows.iter().any(|w| {
                                w.process_name()
                                    .map(|n| n.to_lowercase() == process_name)
                                    .unwrap_or(false)
                                    && w.hwnd().map_or(false, |h| !pre_hwnds.contains(&h))
                            }),
                            LaunchWait::MatchAny => unreachable!(),
                        };
                        if found {
                            break;
                        }
                        if Instant::now() >= deadline {
                            executor.cleanup_depth(depth);
                            executor.dom.set_depth(depth.saturating_sub(1));
                            return Err(AutomataError::Internal(format!(
                                "timed out waiting for '{target}' window (strategy={:?})",
                                launch.wait
                            )));
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }

                if launch.wait_for.is_none() {
                    executor.dom.set_launch_context(LaunchContext {
                        wait: launch.wait,
                        pid,
                        pre_hwnds,
                        process_name,
                    });
                }
            }
        }

        // Push this workflow's global recovery handlers onto the executor stack.
        // They fire for every phase (and all subflows) without per-phase opt-in.
        // Placed after the launch block so early launch-timeout returns don't need cleanup.
        let global_handlers_base = executor.global_handlers.len();
        for (name, h) in &self.global_recovery_handlers {
            executor.global_handlers.push(RecoveryHandler {
                name: name.clone(),
                trigger: h.trigger.clone(),
                actions: h.actions.clone(),
                resume: h.resume,
            });
        }

        // Run each phase.
        // `finally: true` phases run unconditionally even when an earlier phase has failed.
        // Normal phase errors set `workflow_error` and skip remaining normal phases.
        // `flow_control` phases jump to a named phase when their condition is true.
        let mut workflow_error: Option<AutomataError> = None;
        let phases = self.phases;
        let mut i = 0;

        while i < phases.len() {
            // Borrow the phase — never consume. Backward go_to jumps simply set `i`
            // to an earlier index and the phase is re-evaluated on the next iteration.
            let phase = &phases[i];
            i += 1;

            match phase {
                YamlPhase::FlowControl(fc) => {
                    // Flow-control nodes are skipped once a workflow error is set.
                    if workflow_error.is_some() {
                        continue;
                    }
                    log::info!("phase: {} [flow_control]", fc.name);
                    if executor.eval_condition(
                        &fc.flow_control.condition,
                        &state.locals,
                        &state.params,
                        &state.output,
                    )? {
                        log::info!(
                            "phase '{}': condition true → go_to '{}'",
                            fc.name,
                            fc.flow_control.go_to
                        );
                        match phases
                            .iter()
                            .position(|p| p.name() == fc.flow_control.go_to)
                        {
                            Some(idx) => i = idx,
                            None => {
                                let msg = format!(
                                    "phase '{}': go_to target '{}' not found",
                                    fc.name, fc.flow_control.go_to
                                );
                                send!(PhaseEvent::Failed(msg.clone()));
                                return Err(AutomataError::Internal(msg));
                            }
                        }
                    } else {
                        log::info!("phase '{}': condition false, falling through", fc.name);
                    }
                }

                YamlPhase::Subflow(sf) => {
                    if workflow_error.is_some() {
                        continue;
                    }

                    send!(PhaseEvent::PhaseStarted(sf.name.clone()));

                    // Substitute {output.X} in param values.
                    let raw_params: HashMap<String, String> = sf
                        .params
                        .iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                sub_action_output(v, &state.locals, &state.output),
                            )
                        })
                        .collect();

                    let child_path = resolve_subflow_path(&sf.subflow, source_path.as_deref());
                    let child_path_str = child_path.to_string_lossy().to_string();

                    match WorkflowFile::load(&child_path_str, &raw_params) {
                        Err(e) => {
                            let msg = format!("subflow '{}': failed to load: {e}", sf.name);
                            send!(PhaseEvent::PhaseFailed {
                                phase: sf.name.clone(),
                                error: msg.clone(),
                            });
                            workflow_error = Some(AutomataError::Internal(msg));
                        }
                        Ok(child_wf) => {
                            let cb = on_event
                                .as_mut()
                                .map(|f| &mut **f as &mut dyn FnMut(PhaseEvent));
                            match child_wf.run_inner(executor, cb, cancel, depth + 1) {
                                Ok(child_state) => {
                                    state.output.merge(child_state.output);
                                    send!(PhaseEvent::PhaseCompleted(sf.name.clone()));
                                }
                                Err(e) => {
                                    send!(PhaseEvent::PhaseFailed {
                                        phase: sf.name.clone(),
                                        error: e.to_string(),
                                    });
                                    workflow_error = Some(e);
                                }
                            }
                        }
                    }
                    // Restore depth after subflow returns.
                    executor.dom.set_depth(depth);
                }

                YamlPhase::Action(phase) => {
                    // Skip non-finally phases once an error has occurred.
                    if workflow_error.is_some() && !phase.finally {
                        continue;
                    }

                    log::info!(
                        "phase: {}{}",
                        phase.name,
                        if phase.finally { " [finally]" } else { "" }
                    );

                    // Evaluate phase-level precondition before mounting. False → skip phase.
                    if let Some(pre) = &phase.precondition {
                        log::info!("phase '{}': precondition: {}", phase.name, pre.describe());
                        if !executor.eval_condition(
                            pre,
                            &state.locals,
                            &state.params,
                            &state.output,
                        )? {
                            log::info!("phase '{}': skipping (precondition false)", phase.name);
                            send!(PhaseEvent::PhaseSkipped(phase.name.clone()));
                            continue;
                        }
                    }

                    // Mount anchors declared for this phase.
                    if !phase.mount.is_empty() {
                        let defs: Vec<AnchorDef> = phase
                            .mount
                            .iter()
                            .filter_map(|name| {
                                self.anchors
                                    .remove_entry(name)
                                    .map(|(k, v)| v.into_def(k))
                                    .or_else(|| {
                                        log::warn!("anchor '{name}' not found");
                                        None
                                    })
                            })
                            .collect();
                        if !defs.is_empty() {
                            // Retry mount with the phase default timeout.  This handles the
                            // race between `wait: match_any` (which fires the app but does not
                            // poll) and the Root anchor resolution that runs immediately after.
                            // shadow_dom::mount() rolls back a failed def so that the next
                            // attempt can re-register and retry resolution.
                            let mount_deadline = Instant::now() + default_timeout;
                            let mount_result = loop {
                                match executor.mount(defs.clone()) {
                                    Ok(()) => break Ok(()),
                                    Err(e) => {
                                        if Instant::now() >= mount_deadline {
                                            break Err(AutomataError::Internal(format!(
                                                "phase '{}': mount failed: {e}",
                                                phase.name
                                            )));
                                        }
                                        std::thread::sleep(Duration::from_millis(200));
                                    }
                                }
                            };
                            if let Err(e) = mount_result {
                                if phase.finally {
                                    log::warn!("finally phase '{}' mount failed: {e}", phase.name);
                                } else {
                                    workflow_error = Some(e);
                                }
                                continue;
                            }
                        }
                    }

                    // Resolve named recovery handlers.
                    let handlers: Vec<RecoveryHandler> = phase
                        .recovery
                        .as_ref()
                        .map(|r| r.handlers.as_slice())
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|name| {
                            self.recovery_handlers.get(name).map(|h| RecoveryHandler {
                                name: name.clone(),
                                trigger: h.trigger.clone(),
                                actions: h.actions.clone(),
                                resume: h.resume,
                            })
                        })
                        .collect();

                    let max_recoveries = phase
                        .recovery
                        .as_ref()
                        .and_then(|r| r.limit)
                        .or(self.defaults.recovery.limit)
                        .unwrap_or(10);

                    let plan = Plan {
                        name: &phase.name,
                        steps: &phase.steps,
                        recovery_handlers: handlers,
                        max_recoveries,
                        unmount: &phase.unmount,
                        default_timeout,
                        default_retry: default_retry.clone(),
                    };

                    // Check cancellation before starting non-finally phases.
                    if !phase.finally
                        && cancel
                            .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
                            .unwrap_or(false)
                    {
                        workflow_error = Some(AutomataError::Cancelled);
                        continue; // still run finally phases
                    }

                    send!(PhaseEvent::PhaseStarted(phase.name.clone()));
                    match executor.run(&plan, &mut state) {
                        Ok(()) => send!(PhaseEvent::PhaseCompleted(phase.name.clone())),
                        Err(e) => {
                            send!(PhaseEvent::PhaseFailed {
                                phase: phase.name.clone(),
                                error: e.to_string(),
                            });
                            if phase.finally {
                                log::warn!("finally phase '{}' failed: {e}", phase.name);
                                // Finally phase errors are informational only.
                            } else {
                                workflow_error = Some(e);
                            }
                        }
                    }
                }
            }
        }

        // Safety net: clean up any depth-scoped anchors not explicitly unmounted.
        executor.cleanup_depth(depth);
        // Restore depth to parent level.
        executor.dom.set_depth(depth.saturating_sub(1));

        // Pop this workflow's global recovery handlers from the executor stack.
        executor.global_handlers.truncate(global_handlers_base);

        match workflow_error {
            Some(e) => {
                send!(PhaseEvent::Failed(e.to_string()));
                Err(e)
            }
            None => {
                send!(PhaseEvent::Completed);
                Ok(state)
            }
        }
    }
}
