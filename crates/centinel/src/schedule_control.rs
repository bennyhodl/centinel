//! The active schedule configuration shared by the scheduler and browser administration.
//!
//! The path is resolved once at startup. Browser requests never choose it. Edits are
//! serialized, use the core schedule op, and publish a validated snapshot before replying.
//! A running collection finishes with its original arguments; subsequent fires use the
//! latest snapshot. Disabled servers save changes without arming a scheduler.
use std::{
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Result;
use centinel_core::{
    config::Config,
    op::Ctx,
    ops::{self, ScheduleAction, ScheduleArgs},
};
use tokio::sync::{Mutex, Notify};

#[derive(Debug)]
pub struct ScheduleControl {
    path: PathBuf,
    config: RwLock<Config>,
    edits: Mutex<()>,
    pub changed: Notify,
    running: AtomicBool,
    disabled_reason: &'static str,
}

impl ScheduleControl {
    pub fn load(path: Option<&str>, disabled_reason: &'static str) -> Result<Arc<Self>> {
        let explicit = path.is_some();
        let path = match path {
            Some(path) => centinel_core::config::expand_tilde(path),
            None => Config::locate().unwrap_or_else(Config::write_path),
        };
        anyhow::ensure!(
            path.exists() || !explicit,
            "config file {} is missing",
            path.display()
        );
        let config = if path.exists() {
            Config::from_file(&path)?
        } else {
            Config::default()
        };
        Ok(Arc::new(Self {
            path: std::path::absolute(path)?,
            config: RwLock::new(config),
            edits: Mutex::new(()),
            changed: Notify::new(),
            running: AtomicBool::new(false),
            disabled_reason,
        }))
    }

    pub fn snapshot(&self) -> Config {
        self.config.read().unwrap().clone()
    }
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Release);
    }
    pub fn disabled_reason(&self) -> Option<&'static str> {
        (!self.is_running()).then_some(self.disabled_reason)
    }

    fn publish(&self, config: Config) {
        *self.config.write().unwrap() = config;
        self.changed.notify_one();
    }

    pub async fn reload(&self, ctx: &Ctx) -> Result<()> {
        let _edit = self.edits.lock().await;
        let config = Config::from_file(&self.path)?;
        centinel_core::catalog::Sources::new(&ctx.store)
            .check_schedules(&config)
            .await?;
        self.publish(config);
        Ok(())
    }

    pub async fn edit(&self, ctx: &Ctx, action: ScheduleAction) -> Result<()> {
        let _edit = self.edits.lock().await;
        self.edit_locked(ctx, action).await
    }

    pub async fn set_enabled(&self, ctx: &Ctx, id: &str, enabled: bool) -> Result<()> {
        let _edit = self.edits.lock().await;
        let mut schedule = self
            .snapshot()
            .schedule(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no schedule `{id}`"))?;
        schedule.enabled = Some(enabled);
        self.edit_locked(
            ctx,
            ScheduleAction::Set(ops::SetArgs {
                replace: true,
                ..schedule.into()
            }),
        )
        .await
    }

    async fn edit_locked(&self, ctx: &Ctx, mut action: ScheduleAction) -> Result<()> {
        let path = self.path.display().to_string();
        match &mut action {
            ScheduleAction::Set(args) => {
                if args.replace {
                    anyhow::ensure!(
                        self.snapshot()
                            .schedule(args.id.as_deref().unwrap_or(""))
                            .is_some(),
                        "schedule does not exist"
                    );
                }
                args.config = Some(path);
            }
            ScheduleAction::Rm(args) => args.config = Some(path),
        }
        ops::schedule(ctx, ScheduleArgs { action }).await?;
        self.publish(Config::from_file(&self.path)?);
        Ok(())
    }
}
