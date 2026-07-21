use campus_tool_protocol::{
    read_message, write_message, ToolCommand, ToolEvent, ToolKind, PROTOCOL_VERSION,
};
use rand::Rng;
use std::future::Future;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;

#[cfg(target_os = "windows")]
use tokio::net::windows::named_pipe::ServerOptions;

#[derive(Clone)]
pub(crate) struct DesktopToolProcessSupervisor {
    inner: Arc<SupervisorInner>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HelperFaultPoint {
    BeforeSpawn,
    AfterSpawn,
}

struct SupervisorInner {
    children: Mutex<Vec<TrackedChild>>,
    next_fault: Mutex<Option<HelperFaultPoint>>,
}

struct TrackedChild {
    tool: ToolKind,
    child: Child,
}

impl DesktopToolProcessSupervisor {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(SupervisorInner {
                children: Mutex::new(Vec::new()),
                next_fault: Mutex::new(None),
            }),
        }
    }

    pub(crate) fn inject_next_failure(&self, point: HelperFaultPoint) {
        if let Ok(mut fault) = self.inner.next_fault.lock() {
            *fault = Some(point);
        }
    }

    fn take_injected_failure(&self) -> Result<Option<HelperFaultPoint>, String> {
        self.inner
            .next_fault
            .lock()
            .map(|mut fault| fault.take())
            .map_err(|_| "tool fault-injection lock poisoned".into())
    }

    fn reap_finished(&self) -> Result<(), String> {
        let mut children = self
            .inner
            .children
            .lock()
            .map_err(|_| "tool child lock poisoned")?;
        children.retain_mut(|tracked| match tracked.child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => {
                let _ = tracked.child.kill();
                let _ = tracked.child.wait();
                false
            }
        });
        Ok(())
    }

    fn ensure_tool_is_not_running(&self, tool: ToolKind) -> Result<(), String> {
        self.reap_finished()?;
        let children = self
            .inner
            .children
            .lock()
            .map_err(|_| "tool child lock poisoned")?;
        if children.iter().any(|tracked| tracked.tool == tool) {
            Err(format!("{tool:?} helper is already running"))
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    pub(crate) fn active_child_count(&self) -> usize {
        let _ = self.reap_finished();
        self.inner
            .children
            .lock()
            .map_or(0, |children| children.len())
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn launch<H, F>(
        &self,
        executable_name: &str,
        expected_tool: ToolKind,
        open_command: ToolCommand,
        mut handle_event: H,
    ) -> Result<(), String>
    where
        H: FnMut(ToolEvent) -> F + Send + 'static,
        F: Future<Output = ()> + 'static,
    {
        let injected_failure = self.take_injected_failure()?;
        if injected_failure == Some(HelperFaultPoint::BeforeSpawn) {
            return Err("injected helper failure at BeforeSpawn".into());
        }
        self.ensure_tool_is_not_running(expected_tool)?;
        let executable = tool_executable(executable_name)?;
        let random: u128 = rand::rng().random();
        let pipe = format!(r"\\.\pipe\campus-reconstruction-{random:032x}");
        let token = format!("{:032x}", rand::rng().random::<u128>());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let server = {
            let _runtime_context = runtime.enter();
            ServerOptions::new()
                .first_pipe_instance(true)
                .create(&pipe)
                .map_err(|error| error.to_string())?
        };
        let child = Command::new(executable)
            .arg(&pipe)
            .arg(&token)
            .spawn()
            .map_err(|error| error.to_string())?;
        let mut child = child;
        if injected_failure == Some(HelperFaultPoint::AfterSpawn) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("injected helper failure at AfterSpawn".into());
        }
        self.inner
            .children
            .lock()
            .map_err(|_| "tool child lock poisoned")?
            .push(TrackedChild {
                tool: expected_tool,
                child,
            });
        thread::spawn(move || {
            runtime.block_on(async move {
                let result: Result<(), String> = async {
                    let mut server = server;
                    server.connect().await.map_err(|error| error.to_string())?;
                    let hello: ToolCommand = read_message(&mut server).await?;
                    match hello {
                        ToolCommand::Hello {
                            protocol_version,
                            session_token,
                            tool,
                        } if protocol_version == PROTOCOL_VERSION
                            && session_token == token
                            && tool == expected_tool => {}
                        _ => return Err("desktop tool process handshake rejected".into()),
                    }
                    write_message(&mut server, &open_command).await?;
                    loop {
                        let event: ToolEvent = read_message(&mut server).await?;
                        let closed = matches!(event, ToolEvent::Closed { .. });
                        handle_event(event).await;
                        if closed {
                            return Ok(());
                        }
                    }
                }
                .await;
                if let Err(message) = result {
                    handle_event(ToolEvent::Error { message }).await;
                    handle_event(ToolEvent::Closed {
                        tool: expected_tool,
                    })
                    .await;
                }
            });
        });
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn launch<H, F>(
        &self,
        _executable_name: &str,
        _expected_tool: ToolKind,
        _open_command: ToolCommand,
        _handle_event: H,
    ) -> Result<(), String>
    where
        H: FnMut(ToolEvent) -> F + Send + 'static,
        F: Future<Output = ()> + 'static,
    {
        Err("desktop tool processes are currently supported only on Windows".into())
    }
}

impl Drop for SupervisorInner {
    fn drop(&mut self) {
        let Ok(children) = self.children.get_mut() else {
            return;
        };
        for child in children {
            let child = &mut child.child;
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

fn tool_executable(name: &str) -> Result<PathBuf, String> {
    let directory = std::env::current_exe()
        .map_err(|error| error.to_string())?
        .parent()
        .ok_or("native executable has no parent")?
        .to_path_buf();
    let executable = directory.join(format!("{name}.exe"));
    executable
        .exists()
        .then_some(executable)
        .ok_or_else(|| format!("{name}.exe is not installed beside the main application"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_executable_resolution_is_centralized() {
        let missing = tool_executable("definitely-not-a-campus-tool").unwrap_err();
        assert!(missing.contains("is not installed beside the main application"));
    }

    #[test]
    fn injected_pre_spawn_failure_starts_no_helper() {
        let supervisor = DesktopToolProcessSupervisor::new();
        supervisor.inject_next_failure(HelperFaultPoint::BeforeSpawn);
        let error = supervisor
            .launch(
                "definitely-not-a-campus-tool",
                ToolKind::Map,
                ToolCommand::Shutdown,
                |_event| async {},
            )
            .unwrap_err();

        assert!(error.contains("BeforeSpawn"));
        assert_eq!(supervisor.active_child_count(), 0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn injected_post_spawn_failure_reaps_the_helper() {
        let supervisor = DesktopToolProcessSupervisor::new();
        let current = std::env::current_exe().unwrap();
        let executable_name = current.file_stem().unwrap().to_string_lossy().into_owned();
        supervisor.inject_next_failure(HelperFaultPoint::AfterSpawn);
        let error = supervisor
            .launch(
                &executable_name,
                ToolKind::Preview,
                ToolCommand::Shutdown,
                |_event| async {},
            )
            .unwrap_err();

        assert!(error.contains("AfterSpawn"));
        assert_eq!(supervisor.active_child_count(), 0);
    }
}
