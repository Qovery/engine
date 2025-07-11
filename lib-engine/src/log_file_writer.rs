use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::{io, mem};

#[derive(Clone, Default)]
pub struct LogFileWriter {
    file: Arc<(AtomicBool, Mutex<Option<BufWriter<File>>>)>,
}

pub struct LogFileWriterInner {
    file: Arc<(AtomicBool, Mutex<Option<BufWriter<File>>>)>,
    writer: Option<BufWriter<File>>,
}

impl LogFileWriter {
    pub fn new() -> (Self, LogFileWriterInner) {
        let file = Arc::new((AtomicBool::new(false), Mutex::new(None)));
        (Self { file: file.clone() }, LogFileWriterInner { file, writer: None })
    }

    pub fn enable(&self, directory: &Path) {
        if let Ok(mut file) = self.lock_file() {
            let file_path = directory.join("engine.log");
            match File::create(file_path) {
                Ok(new_file) => {
                    let new_file = BufWriter::new(new_file);
                    *file = Some(new_file);
                    self.file.0.store(true, Ordering::Relaxed)
                }
                Err(_) => *file = None,
            }
        }
    }

    pub fn disable(&self) {
        if let Ok(mut file) = self.lock_file() {
            *file = None;
            self.file.0.store(true, Ordering::Relaxed);

            // Trigger a write call to disable the current file in the LogWriter
            debug!("disable log file writer");
        }
    }

    fn lock_file(&self) -> io::Result<MutexGuard<Option<BufWriter<File>>>> {
        self.file.1.lock().map_err(|_| io::Error::other("Mutex lock failed"))
    }
}

impl LogFileWriterInner {
    fn file_has_changed(&self) -> bool {
        self.file.0.swap(false, Ordering::Relaxed)
    }

    fn update_file_if_necessary(&mut self) -> io::Result<()> {
        if self.file_has_changed() {
            let mut guard = self.file.1.lock().map_err(|_| io::Error::other("Mutex lock failed"))?;
            self.writer = mem::take(&mut *guard);
        }
        Ok(())
    }
}

impl Write for LogFileWriterInner {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.update_file_if_necessary()?;
        if let Some(file) = self.writer.as_mut() {
            file.write_all(buf)?;
        }
        // Always returns OK whatever the FileLogger is enabled or not.
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.update_file_if_necessary()?;
        if let Some(file) = self.writer.as_mut() {
            file.flush()
        } else {
            Ok(())
        }
    }
}
