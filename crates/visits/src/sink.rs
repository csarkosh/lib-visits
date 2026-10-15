//! Where decorated events go: one JSON object a line. A sink's failure is
//! the operator's to read on stderr; the browser still gets its 204.
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::sync::Mutex;
use visits_core::Event;

pub trait Sink: Send + Sync {
    fn write(&self, events: &[Event]) -> io::Result<()>;
}

fn lines(events: &[Event]) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    for e in events {
        serde_json::to_writer(&mut buf, e).map_err(io::Error::other)?;
        buf.push(b'\n');
    }
    Ok(buf)
}

pub struct StdoutSink;
impl Sink for StdoutSink {
    fn write(&self, events: &[Event]) -> io::Result<()> {
        let buf = lines(events)?;
        let mut out = io::stdout().lock();
        out.write_all(&buf)?;
        out.flush()
    }
}

pub struct FileSink(Mutex<File>);
impl Sink for FileSink {
    fn write(&self, events: &[Event]) -> io::Result<()> {
        let buf = lines(events)?;
        let mut f = self.0.lock().map_err(|_| io::Error::other("sink poisoned"))?;
        f.write_all(&buf)
    }
}

pub fn sink_from(spec: &str) -> anyhow::Result<Box<dyn Sink>> {
    match spec {
        "stdout" => Ok(Box::new(StdoutSink)),
        s if s.starts_with("file:") => {
            let f = OpenOptions::new().create(true).append(true).open(&s[5..])?;
            Ok(Box::new(FileSink(Mutex::new(f))))
        }
        other => anyhow::bail!("unknown sink {other:?}: use stdout or file:<path>"),
    }
}
