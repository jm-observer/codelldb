use crate::prelude::*;

use crate::dap_session::DAPSession;
use adapter_protocol::*;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::net::{TcpListener, TcpStream};

pub struct Terminal {
    #[allow(unused)]
    connection: TcpStream,
    data: String,
}

impl Terminal {
    pub async fn create(
        terminal_kind: impl Into<String>,
        title: impl Into<String>,
        clear_sequence: Option<Vec<String>>,
        dap_session: DAPSession,
    ) -> Result<Terminal, Error> {
        let terminal_kind = terminal_kind.into();
        let title = title.into();

        let terminal_fut = async move {
            if let Some(clear_sequence) = clear_sequence {
                let req_args = RunInTerminalRequestArguments {
                    args: clear_sequence,
                    cwd: String::new(),
                    env: None,
                    kind: Some(terminal_kind.clone()),
                    title: Some(title.clone()),
                };
                dap_session.send_request(RequestArguments::runInTerminal(req_args)).await?;
            }

            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let addr = listener.local_addr()?;

            let accept_fut = listener.accept();

            // Run codelldb in a terminal agent mode, which sends back the tty device name (Unix)
            // or its own process id (Windows), then waits till the socket gets closed from our end.
            let executable = std::env::current_exe()?.to_str().unwrap().into();
            let args = vec![
                executable,
                "terminal-agent".into(),
                format!("--connect={}", addr.port()),
            ];
            let req_args = RunInTerminalRequestArguments {
                args: args,
                cwd: String::new(),
                env: None,
                kind: Some(terminal_kind),
                title: Some(title),
            };

            tokio::spawn(async move {
                let response = dap_session.send_request(RequestArguments::runInTerminal(req_args));
                log_errors!(response.await);
            });

            let (stream, _remote_addr) = accept_fut.await?;
            let mut reader = BufReader::new(stream);
            let mut data = String::new();
            reader.read_line(&mut data).await?;

            Ok(Terminal {
                connection: reader.into_inner(),
                data: data.trim().to_owned(),
            })
        };

        match tokio::time::timeout(Duration::from_secs(300), terminal_fut).await {
            Ok(res) => res,
            Err(_) => bail!("Terminal agent did not respond within the allotted time."),
        }
    }

    pub fn input_devname(&self) -> &str {
        if cfg!(windows) {
            "CONIN$"
        } else {
            &self.data
        }
    }

    pub fn output_devname(&self) -> &str {
        if cfg!(windows) {
            "CONOUT$"
        } else {
            &self.data
        }
    }

    #[cfg(windows)]
    pub fn attach_console(&self) {
        unsafe {
            let pid = self.data.parse::<u32>().unwrap();
            let rs_code = winapi::um::wincon::FreeConsole();
            if rs_code == 0 {
                let error_code = winapi::um::errhandlingapi::GetLastError();
                error!("FreeConsole failed with error code: {}", error_code);
            }
            let rs_code = winapi::um::wincon::AttachConsole(pid);
            if rs_code == 0 {
                let error_code = winapi::um::errhandlingapi::GetLastError();
                error!("AttachConsole failed with error code: {}", error_code);
            }
        }
    }

    #[cfg(windows)]
    pub fn detach_console(&self) {
        unsafe {
            dbg!(winapi::um::wincon::FreeConsole());
        }
    }
}
