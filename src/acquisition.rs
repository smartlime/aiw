use std::error::Error;
use std::fmt;
use std::io::{self, IsTerminal, Write};
use std::process::Command;

use crate::registry::Acquisition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquisitionError(String);

impl AcquisitionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for AcquisitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for AcquisitionError {}

pub trait TokenAcquirer {
    fn acquire(
        &self,
        acquisition: &Acquisition,
        names: &[&str],
    ) -> Result<Vec<u8>, AcquisitionError>;
}

#[derive(Default)]
pub struct BrowserTokenAcquirer;

impl TokenAcquirer for BrowserTokenAcquirer {
    fn acquire(
        &self,
        acquisition: &Acquisition,
        names: &[&str],
    ) -> Result<Vec<u8>, AcquisitionError> {
        let status = Command::new("open")
            .arg(&acquisition.url)
            .status()
            .map_err(|error| AcquisitionError::new(format!("cannot open browser: {error}")))?;
        if !status.success() {
            return Err(AcquisitionError::new(format!(
                "browser opener exited with status {status}"
            )));
        }

        if io::stderr().is_terminal() {
            eprintln!("\u{1b}[36m◇\u{1b}[0m Token source: {}", acquisition.url);
        } else {
            eprintln!("Token source: {}", acquisition.url);
        }
        let prompt = if io::stdout().is_terminal() {
            format!(
                "\u{1b}[36m◆\u{1b}[0m Paste token for {}: ",
                names.join(", ")
            )
        } else {
            format!("Paste token for {}: ", names.join(", "))
        };
        read_hidden_line(&prompt)
            .map(String::into_bytes)
            .map_err(|error| AcquisitionError::new(format!("cannot read token: {error}")))
    }
}

fn read_hidden_line(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let _guard = EchoGuard::disable()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    eprintln!();
    while matches!(value.as_bytes().last(), Some(b'\n' | b'\r')) {
        value.pop();
    }
    Ok(value)
}

#[cfg(unix)]
struct EchoGuard {
    fd: libc::c_int,
    original: Option<libc::termios>,
}

#[cfg(unix)]
impl EchoGuard {
    fn disable() -> io::Result<Self> {
        use std::mem::MaybeUninit;
        use std::os::fd::AsRawFd;

        let fd = io::stdin().as_raw_fd();
        let mut original = MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENOTTY) {
                return Ok(Self { fd, original: None });
            }
            return Err(error);
        }
        let original = unsafe { original.assume_init() };
        let mut hidden = original;
        hidden.c_lflag &= !libc::ECHO;
        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &hidden) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            fd,
            original: Some(original),
        })
    }
}

#[cfg(unix)]
impl Drop for EchoGuard {
    fn drop(&mut self) {
        if let Some(original) = &self.original {
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSAFLUSH, original);
            }
        }
    }
}

#[cfg(not(unix))]
struct EchoGuard;

#[cfg(not(unix))]
impl EchoGuard {
    fn disable() -> io::Result<Self> {
        Ok(Self)
    }
}
