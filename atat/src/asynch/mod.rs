mod client;
mod simple_client;

pub use client::Client;
pub use simple_client::SimpleClient;

use crate::{AtatCmd, Error};
use embassy_time::Duration;

#[derive(Clone, Copy)]
struct AtatCmdSpec<'a> {
    timeout: Duration,
    expects_response_code: bool,
    expects_prompt: bool,
    payload: &'a [u8],
}

impl<'a, Cmd> From<&'a Cmd> for AtatCmdSpec<'a>
where
    Cmd: AtatCmd + ?Sized,
{
    fn from(cmd: &'a Cmd) -> Self {
        Self {
            timeout: Duration::from_millis(Cmd::MAX_TIMEOUT_MS.into()),
            expects_response_code: Cmd::EXPECTS_RESPONSE_CODE,
            expects_prompt: Cmd::EXPECTS_PROMPT,
            payload: cmd.payload(),
        }
    }
}

pub trait AtatClient {
    /// Send an AT command.
    ///
    /// `cmd` must implement [`AtatCmd`].
    ///
    /// This function will also make sure that at least `self.config.cmd_cooldown`
    /// has passed since the last response or URC has been received, to allow
    /// the slave AT device time to deliver URC's.
    async fn send<Cmd: AtatCmd>(&mut self, cmd: &Cmd) -> Result<Cmd::Response, Error>;

    async fn send_retry<Cmd: AtatCmd>(&mut self, cmd: &Cmd) -> Result<Cmd::Response, Error> {
        for attempt in 1..=Cmd::ATTEMPTS {
            if attempt > 1 {
                debug!("Attempt {}:", attempt);
            }

            match self.send(cmd).await {
                Err(Error::Timeout) => {}
                Err(Error::Parse) => {
                    if !Cmd::REATTEMPT_ON_PARSE_ERR {
                        return Err(Error::Parse);
                    }
                }
                r => return r,
            }
        }
        Err(Error::Timeout)
    }
}

impl<T> AtatClient for &mut T
where
    T: AtatClient,
{
    async fn send<Cmd: AtatCmd>(&mut self, cmd: &Cmd) -> Result<Cmd::Response, Error> {
        T::send(self, cmd).await
    }
}
