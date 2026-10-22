//! The same command host over a byte pipe, including an initial version check.
use crate::Host;
use serde_json::{Map, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use xirius_core::{
    CmdError, Registry,
    protocol::{Hello, Request, Response},
    session::{RunOptions, RunResult},
};

pub fn serve(host: &dyn Host, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    let mut connected = false;
    for line in input.lines() {
        let request: Request = serde_json::from_str(&line?)?;
        let response = match request {
            Request::Hello(hello) => match hello.check() {
                Ok(()) => {
                    connected = true;
                    Response::Hello(Hello::default())
                }
                Err(error) => {
                    connected = false;
                    Response::Error(error)
                }
            },
            Request::Run {
                command,
                args,
                options,
                save,
            } if connected => match host.run(&command, &args, &options, save) {
                Ok(result) => Response::Result(Box::new(result)),
                Err(error) => Response::Error(error),
            },
            Request::Run { .. } => {
                Response::Error(CmdError::conflict("接続時のプロトコル照合が必要です"))
            }
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

struct Pipe {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Pipe {
    fn exchange(&mut self, request: Request) -> Result<Response, CmdError> {
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| CmdError::io("engine への接続が閉じられています"))?;
        serde_json::to_writer(&mut *input, &request).map_err(|e| CmdError::io(e.to_string()))?;
        input.write_all(b"\n")?;
        input.flush()?;
        let mut line = String::new();
        if self.output.read_line(&mut line)? == 0 {
            return Err(CmdError::io("engine の接続が閉じられました"));
        }
        serde_json::from_str(&line).map_err(|e| CmdError::io(e.to_string()))
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        self.input.take();
        // Closing stdin asks the host to end; reap it before returning.
        let _ = self.child.wait();
    }
}

pub struct PipeHost {
    registry: Registry,
    pipe: Mutex<Pipe>,
}

impl PipeHost {
    pub fn start(mut command: Command, registry: Registry, hello: Hello) -> Result<Self, CmdError> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn()?;
        let mut pipe = Pipe {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
        };
        match pipe.exchange(Request::Hello(hello))? {
            Response::Hello(hello) => hello.check()?,
            Response::Error(error) => return Err(error),
            _ => return Err(CmdError::conflict("engine の接続応答を読み取れません")),
        }
        Ok(Self {
            registry,
            pipe: Mutex::new(pipe),
        })
    }
}

impl Host for PipeHost {
    fn with_registry(&self, f: &mut dyn FnMut(&Registry)) {
        f(&self.registry);
    }
    fn run(
        &self,
        command: &str,
        args: &Map<String, Value>,
        options: &RunOptions,
        save: bool,
    ) -> Result<RunResult, CmdError> {
        match self
            .pipe
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exchange(Request::Run {
                command: command.into(),
                args: args.clone(),
                options: options.clone(),
                save,
            })? {
            Response::Result(result) => Ok(*result),
            Response::Error(error) => Err(error),
            _ => Err(CmdError::conflict("engine の結果を読み取れません")),
        }
    }
}
