use super::CompiledAsset;
use std::io;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};

type Job = Box<dyn FnOnce() + Send>;

pub(super) struct Encoders {
    jobs: Option<mpsc::Sender<Job>>,
    workers: Vec<JoinHandle<()>>,
}

pub(crate) fn worker_count() -> io::Result<usize> {
    let Some(value) = std::env::var_os("BAB_ASSET_WORKERS") else {
        return Ok(1);
    };
    value
        .to_str()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| (1..=64).contains(count))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "BAB_ASSET_WORKERS must be between 1 and 64",
            )
        })
}

impl Encoders {
    pub(super) fn start() -> io::Result<Option<Self>> {
        let count = worker_count()?;
        if count == 1 {
            return Ok(None);
        }

        let (jobs, queue) = mpsc::channel::<Job>();
        let queue = Arc::new(Mutex::new(queue));
        let mut encoders = Self {
            jobs: Some(jobs),
            workers: Vec::with_capacity(count),
        };
        for index in 0..count {
            let queue = Arc::clone(&queue);
            encoders.workers.push(
                thread::Builder::new()
                    .name(format!("asset-encoder-{index}"))
                    .spawn(move || {
                        loop {
                            let job = queue.lock().unwrap().recv();
                            let Ok(job) = job else { break };
                            job();
                        }
                    })?,
            );
        }
        Ok(Some(encoders))
    }

    pub(super) fn capacity(&self) -> usize {
        self.workers.len() * 2
    }

    pub(super) fn submit(
        &self,
        encode: impl FnOnce() -> io::Result<CompiledAsset> + Send + 'static,
    ) -> io::Result<mpsc::Receiver<io::Result<CompiledAsset>>> {
        let (output, result) = mpsc::channel();
        self.jobs
            .as_ref()
            .unwrap()
            .send(Box::new(move || {
                let _ = output.send(encode());
            }))
            .map_err(|_| io::Error::other("asset encoder queue closed"))?;
        Ok(result)
    }
}

impl Drop for Encoders {
    fn drop(&mut self) {
        drop(self.jobs.take());
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
