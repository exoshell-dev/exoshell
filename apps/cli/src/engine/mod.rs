mod prelude;
mod run;
use self::prelude::*;

#[derive(Debug)]
pub struct Engine {
  pub db: Arc<Database>,
  pub children: Arc<RwLock<HashMap<String, Arc<Mutex<mpsc::UnboundedSender<ChildCommand>>>>>>,
  pub task_tracker: TaskTracker,
}

#[allow(dead_code)]
pub enum ChildCommand {
  Kill,
}

impl Engine {
  pub async fn new(filepath: impl AsRef<Path>) -> Result<Self> {
    let filepath = filepath.as_ref();
    Ok(Self {
      db: Arc::new(
        Database::new(
          filepath
            .to_str()
            .with_context(|| format!("database path is not valid utf-8: {filepath:#?}"))?,
          None,
          None,
        )
        .await?,
      ),
      children: Arc::new(RwLock::new(HashMap::new())),
      task_tracker: TaskTracker::new(),
    })
  }

  #[instrument(skip(self), err)]
  pub async fn quit(&self) -> Result<()> {
    self.task_tracker.close();
    self.task_tracker.wait().await;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  impl Engine {
    pub async fn new_test_engine() -> Result<Self> {
      Ok(Self {
        db: Arc::new(Database::memory().await?),
        children: Arc::new(RwLock::new(HashMap::new())),
        task_tracker: TaskTracker::new(),
      })
    }
  }
}