//! Keep stdio probes isolated from the caller's process tree and stderr.
use process_wrap::tokio::{ChildWrapper, CommandWrap, CommandWrapper, KillOnDrop};
use rmcp::transport::TokioChildProcess;
use std::{io, process::Stdio};

#[derive(Debug)]
struct KillTreeOnDrop;
impl CommandWrapper for KillTreeOnDrop {
    fn wrap_child(
        &mut self,
        inner: Box<dyn ChildWrapper>,
        _command: &CommandWrap,
    ) -> io::Result<Box<dyn ChildWrapper>> {
        Ok(Box::new(KillTreeChild(Some(inner))))
    }
}
#[derive(Debug)]
struct KillTreeChild(Option<Box<dyn ChildWrapper>>);
impl ChildWrapper for KillTreeChild {
    fn inner(&self) -> &dyn ChildWrapper {
        self.0.as_deref().expect("owned subprocess")
    }
    fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
        self.0.as_deref_mut().expect("owned subprocess")
    }
    fn into_inner(mut self: Box<Self>) -> Box<dyn ChildWrapper> {
        self.0.take().expect("owned subprocess")
    }
}
impl Drop for KillTreeChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // rmcp schedules async cleanup on Drop. A one-shot CLI can tear down its runtime
            // before that task runs, so signal the whole group/job synchronously as well.
            let _ = child.start_kill();
        }
    }
}

pub(crate) fn transport(command: tokio::process::Command) -> io::Result<TokioChildProcess> {
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(process_wrap::tokio::JobObject);
    command.wrap(KillTreeOnDrop);
    // The SDK's builder overrides Stdio configured on the raw Command. Set it here,
    // otherwise a server can echo credentials into the CLI's JSON error stream.
    Ok(TokioChildProcess::builder(command)
        .stderr(Stdio::null())
        .spawn()?
        .0)
}
