#[derive(Debug)]
pub struct SyncError<T: std::error::Error>(std::sync::Mutex<T>);
impl<T: std::error::Error> SyncError<T> {
    pub fn new(value: T) -> SyncError<T> {
        SyncError(std::sync::Mutex::new(value))
    }
}
impl<T: std::error::Error + std::fmt::Display> std::fmt::Display for SyncError<T> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self.0.lock() {
            Ok(value) => {
                value.fmt(fmt)?;
            }
            Err(err) => {
                panic!("Can't acquire lock: {}", err);
            }
        }
        Ok(())
    }
}
impl<T: std::error::Error> std::error::Error for SyncError<T> {}
