#[cfg(windows)]
mod job;

use std::io;
use std::ops::{Deref, DerefMut};
use std::process::Output;
#[cfg(windows)]
use std::sync::Arc;

use tokio::process::{Child, Command};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct KillOnDropChild {
    child: Child,
    #[cfg(windows)]
    job: Arc<job::Job>,
}

pub fn spawn_in_job(command: &mut Command) -> io::Result<KillOnDropChild> {
    command.kill_on_drop(true);
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
        let job = job::Job::new()?;
        let mut child = command.spawn()?;
        if let Err(error) = job.assign(&child) {
            let _ = child.start_kill();
            return Err(error);
        }
        Ok(KillOnDropChild {
            child,
            job: Arc::new(job),
        })
    }
    #[cfg(not(windows))]
    {
        Ok(KillOnDropChild {
            child: command.spawn()?,
        })
    }
}

impl KillOnDropChild {
    pub async fn wait_with_output(self) -> io::Result<Output> {
        let output = self.child.wait_with_output().await;
        #[cfg(windows)]
        drop(self.job);
        output
    }

    pub fn kill_tree(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        self.job.terminate()?;
        self.child.start_kill()
    }
}

impl Deref for KillOnDropChild {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.child
    }
}

impl DerefMut for KillOnDropChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}
