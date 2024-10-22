use std::{mem, sync::{Mutex, MutexGuard}};

pub struct Promise<T>(Mutex<Result<T, Status>>);

#[derive(Copy, Clone, Debug)]
pub enum Status {
    Waiting,
    Broken,
    Taken,
}

pub enum Error {
    WrongState(Status),
    Poisoned,
    AlreadyFull,
}

impl<T> Promise<T> {
    pub fn new() -> Self {
        Self (
            Mutex::new(Err(Status::Waiting))
        )
    }

    fn lock(&self) -> Result<MutexGuard<Result<T, Status>>, Error> {
        self.0
            .lock()
            .map_err(|_| Error::Poisoned)
    }

    pub fn set(&self, value: T) -> Result<(), Error> {
        let mut guard = self.lock()?;
        let data = &mut *guard;
        match data {
            Err(Status::Waiting) => {
                // Valid, write
                *data = Ok(value);
                Ok(())
            }
            Ok(_) => Err(Error::AlreadyFull),
            Err(status) => Err(Error::WrongState(*status)),
        }
    }

    pub fn try_get(&self) -> Result<T, Error> {
        let mut guard = self.lock()?;
        let data = &mut *guard;
        match data {
            Ok(_) => {
                // Get the value and clear it
                let value = mem::replace(data, Err(Status::Taken));
                Ok(value.unwrap())
            }
            Err(status) => {
                Err(Error::WrongState(*status))
            }
        }
    }
}
