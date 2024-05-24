use super::prelude::*;

impl super::Engine {
  #[instrument(skip(self), ret, err)]
  pub async fn run_script(&self, script: Script) -> Result<ScriptRun> {
    let mut mux = AsyncMux::new()?;
    let (out_tag, out_sender) = mux.make_sender()?;
    let (err_tag, err_sender) = mux.make_sender()?;

    let mut child = Command::new(&script.command)
      .args(&script.args)
      .stdout(out_sender)
      .stderr(err_sender)
      .spawn()
      .context("Failed to spawn child process")?;

    let script_run = self
      .db
      .upsert_script_run(
        &ScriptRun::builder()
          .script(script)
          .spawned_at(Utc::now())
          .build()?,
      )
      .await?;

    let id = &script_run.id.as_ref().unwrap().id;

    let (tx, mut rx) = mpsc::unbounded_channel::<ChildCommand>();

    {
      let id = id.clone();
      let db = self.db.clone();

      tokio::spawn(async move {
        loop {
          tokio::select! {
            buf = mux.read() => {
              match buf {
                Ok(buf) => {
                  if buf.tag == out_tag {
                    db.append_script_run_log(
                      &id.clone(),
                      &ScriptRunLog::Stdout {
                        txt: String::from_utf8_lossy(buf.data).to_string(),
                        ts: Utc::now(),
                      },
                    )
                    .await?;
                  } else if buf.tag == err_tag {
                    db.append_script_run_log(
                      &id.clone(),
                      &ScriptRunLog::Stderr {
                        txt: String::from_utf8_lossy(buf.data).to_string(),
                        ts: Utc::now(),
                      },
                    )
                    .await?;
                  }
                },
                Err(e) => {
                  error!("Error reading from mux: {}", e);
                },
              }
            }
            exit_status = child.wait() => {
              debug!("Exit status: {:#?}", exit_status);
              let mut q = db.db.query("UPDATE type::thing('script_run', $id) SET finishedAt = $finishedAt RETURN NONE")
              .bind(("id", id.to_raw()))
              .bind(("finishedAt", Utc::now()));
              match exit_status {
                Ok(status) => {
                  if let Some(exit_code) = status.code() {
                    q = q.query("UPDATE type::thing('script_run', $id) SET exitStatus = $exitStatus RETURN NONE")
                     .bind(("id", id.to_raw()))
                     .bind(("exitStatus", ExitStatus::ExitCode(exit_code)));
                  } else {
                  #[cfg(target_family = "unix")]
                  if let Some(signal) = status.signal() {
                    q = q.query("UPDATE type::thing('script_run', $id) SET exitStatus = $exitStatus RETURN NONE")
                      .bind(("id", id.to_raw()))
                      .bind(("exitStatus", ExitStatus::Signal(signal)));
                  }
                }
                }
                Err(e) => {
                  error!("Error waiting for child: {}", e);
                },
              }
              q.await?;
                while let Some(buf) = mux.read_nonblock()? {
                  if buf.tag == out_tag {
                    db.append_script_run_log(
                      &id.clone(),
                      &ScriptRunLog::Stdout {
                        txt: String::from_utf8_lossy(buf.data).to_string(),
                        ts: Utc::now(),
                      },
                    )
                    .await?;
                  } else if buf.tag == err_tag {
                    db.append_script_run_log(
                      &id.clone(),
                      &ScriptRunLog::Stderr {
                        txt: String::from_utf8_lossy(buf.data).to_string(),
                        ts: Utc::now(),
                      },
                    )
                    .await?;
                  }
                }
              break;
            }
            Some(cmd) = rx.recv() => {
              match cmd {
                ChildCommand::Kill => {
                  child.kill().await?;
                }
              }
            }
          }
        }
        Ok::<(), anyhow::Error>(())
      });
    }

    self
      .children
      .write()
      .await
      .insert(id.clone().to_string(), Arc::new(Mutex::new(tx)));

    Ok(script_run)
  }

  #[instrument(skip(self), err)]
  pub async fn kill_script(&self, id: &str) -> Result<()> {
    let children = self.children.read().await;
    let child = children
      .get(id)
      .ok_or_else(|| anyhow!("Child not found: {id}"))?
      .lock()
      .expect("mutex poisoned");
    child.send(ChildCommand::Kill)?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn test_run_script() -> Result<()> {
    let _engine = Engine::new_test_engine().await?;

    Ok(())
  }
}
