use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch, Mutex, RwLock};
use super::*;

// ---------- Менеджер процесса ----------

/// Запущенный надзор: канал остановки и задача, завершения которой ждём при остановке.
struct Supervisor {
    stop_tx: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

struct ClientLogTasks(Vec<tokio::task::JoinHandle<()>>);

impl ClientLogTasks {
    async fn stop(&mut self) {
        for task in &self.0 { task.abort(); }
        for task in self.0.drain(..) { let _ = task.await; }
    }
}

impl Drop for ClientLogTasks {
    fn drop(&mut self) {
        for task in &self.0 { task.abort(); }
    }
}

struct FdHandoff {
    cancel: Arc<std::sync::atomic::AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum HandoffDecision { Wait, Start, Restart }

pub(super) fn handoff_decision(configured: bool, attached: bool, task_running: bool, failures: u8, retry_ready: bool) -> HandoffDecision {
    if !configured || attached || task_running { return HandoffDecision::Wait; }
    if failures >= 3 { return HandoffDecision::Restart; }
    if retry_ready { HandoffDecision::Start } else { HandoffDecision::Wait }
}

struct HandoffRecovery {
    configured: bool,
    failures: u8,
    retry_at: Instant,
}

impl HandoffRecovery {
    fn new() -> Self { Self { configured: false, failures: 0, retry_at: Instant::now() } }
    fn failed(&mut self) {
        self.failures = self.failures.saturating_add(1);
        self.retry_at = Instant::now() + Duration::from_secs(1 << self.failures.min(3));
    }
}

impl FdHandoff {
    async fn stop(mut self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Release);
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

impl Drop for FdHandoff {
    fn drop(&mut self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Release);
    }
}


pub struct VkTurnManager {
    pub rt: RwLock<VkTurnRuntime>,
    /// Сериализует изменение VK-настроек целиком — от чтения актуального конфига до перезапуска.
    /// Без этого два сохранения могли применить конфиг в одном порядке, а запустить процессы в другом.
    pub config_update_lock: Mutex<()>,
    /// Сериализует restart/stop целиком: два одновременных перезапуска не могут оставить
    /// «осиротевший» надзор и второй клиент.
    op_lock: Mutex<Option<Supervisor>>,
    /// Путь к бинарнику клиента (в тестах подменяется).
    core_path: std::path::PathBuf,
    /// Число живых задач надзора — для диагностики и анти-регрессионного теста гонки перезапусков.
    active_supervisors: Arc<std::sync::atomic::AtomicUsize>,
    /// Фоновая установка ядра: скачивание идёт дольше таймаута запроса веб-панели (30 с).
    pub install: Arc<std::sync::Mutex<CoreInstall>>,
}

/// Состояние фоновой установки ядра для статуса VK-туннеля.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct CoreInstall {
    pub running: bool,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
}

/// Учитывает живую задачу надзора: +1 при создании, −1 при любом выходе (в том числе при abort,
/// даже если задача ни разу не успела выполниться — guard живёт внутри future и падает вместе с ним).
struct SupervisorGuard(Arc<std::sync::atomic::AtomicUsize>);
impl SupervisorGuard {
    fn new(counter: &Arc<std::sync::atomic::AtomicUsize>) -> Self {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(counter.clone())
    }
}
impl Drop for SupervisorGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Состояния, которые объясняют паузу пользователю и не должны затираться на «Переподключаюсь».
pub fn keeps_phase_during_backoff(phase: VkTurnPhase) -> bool {
    is_sticky(phase)
}

/// Что делать после завершения клиента.
#[derive(Debug, PartialEq)]
pub struct ExitDecision {
    pub wait: Duration,
    pub notice: Option<&'static str>,
}

const LONG_PAUSE: Duration = Duration::from_secs(600);

/// Чистая функция решения после выхода клиента (покрыта тестами).
/// Все ссылки клиент использует одновременно, поэтому переключать нечего: при окончательной
/// ошибке — длинная пауза, чтобы не долбить ВК и сервер. Уведомление — один раз на серию сбоев.
pub fn decide_after_exit(
    phase: VkTurnPhase,
    ran_for: Duration,
    backoff: &mut Duration,
    last_notice: &mut Option<VkTurnPhase>,
) -> ExitDecision {
    // Долгая успешная работа — новая серия: сбрасываем паузу и «уже предупреждали».
    if ran_for > Duration::from_secs(300) {
        *backoff = Duration::from_secs(5);
        *last_notice = None;
    }
    let mut notice_once = |p: VkTurnPhase, text: &'static str| -> Option<&'static str> {
        if *last_notice == Some(p) {
            None
        } else {
            *last_notice = Some(p);
            Some(text)
        }
    };
    match phase {
        VkTurnPhase::CaptchaRequired => ExitDecision {
            wait: LONG_PAUSE,
            notice: notice_once(phase, "📞 VK-туннель: ВК запросил капчу, автоматически пройти не удалось. Повторяю попытки раз в 10 минут."),
        },
        VkTurnPhase::LinkInvalid => ExitDecision {
            wait: LONG_PAUSE,
            notice: notice_once(phase, "📞 VK-туннель: ссылка на звонок ВК не работает. Добавьте новую ссылку в панели."),
        },
        VkTurnPhase::AuthFailed => ExitDecision {
            wait: LONG_PAUSE,
            notice: notice_once(phase, "📞 VK-туннель: сервер на VPS отклонил пароль подключения. Проверьте пароль в панели."),
        },
        _ => {
            if ran_for <= Duration::from_secs(300) {
                *backoff = (*backoff * 2).min(Duration::from_secs(120));
            }
            ExitDecision { wait: *backoff, notice: None }
        }
    }
}

impl VkTurnManager {
    pub fn new() -> Arc<Self> {
        Self::with_core_path(CORE_PATH)
    }

    pub fn with_core_path(path: impl Into<std::path::PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            rt: RwLock::new(VkTurnRuntime::default()),
            config_update_lock: Mutex::new(()),
            op_lock: Mutex::new(None),
            core_path: path.into(),
            active_supervisors: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            install: Arc::new(std::sync::Mutex::new(CoreInstall::default())),
        })
    }

    pub fn install_state(&self) -> CoreInstall {
        self.install.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Помечает начало установки. `false` — установка уже идёт (повторное нажатие не запускает вторую).
    pub fn begin_install(&self) -> bool {
        let Ok(mut g) = self.install.lock() else { return false };
        if g.running {
            return false;
        }
        *g = CoreInstall { running: true, ..Default::default() };
        true
    }

    pub fn finish_install(&self, result: &Result<String, String>) {
        if let Ok(mut g) = self.install.lock() {
            g.running = false;
            g.error = result.as_ref().err().cloned();
        }
    }

    pub fn active_supervisors(&self) -> usize {
        self.active_supervisors.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn core_installed() -> bool {
        std::path::Path::new(CORE_PATH).exists()
    }

    /// Идемпотентно: останавливает текущий процесс и запускает надзор с новым конфигом.
    pub async fn restart(self: &Arc<Self>, cfg: VkTurnConfig, state: crate::AppState) {
        let mut slot = self.op_lock.lock().await;
        self.stop_locked(&mut slot).await;
        // Старые исключения снимаются всегда: адрес VPS мог смениться. Надзор поставит актуальные.
        remove_exemptions().await;
        let has_token = selected_vk_token(&cfg).is_some();
        if !cfg.enabled || (!has_token && call_hashes(&cfg).is_empty()) || cfg.password.is_empty() || cfg.peer.is_empty() {
            self.rt.write().await.phase = VkTurnPhase::Disabled;
            return;
        }
        if !self.core_path.exists() {
            self.rt.write().await.phase = VkTurnPhase::NotInstalled;
            return;
        }
        kill_stale_from_pidfile().await;
        let (stop_tx, rx) = watch::channel(false);
        let me = self.clone();
        let alive = SupervisorGuard::new(&self.active_supervisors);
        let task = tokio::spawn(async move {
            let _alive = alive;
            me.supervise(cfg, rx, state).await
        });
        *slot = Some(Supervisor { stop_tx, task });
    }

    pub async fn stop(&self) {
        let mut slot = self.op_lock.lock().await;
        self.stop_locked(&mut slot).await;
        remove_exemptions().await;
    }

    /// Останавливает надзор и ЖДЁТ его завершения (процесс клиента убит, TUN закрыт до выхода из функции).
    async fn stop_locked(&self, slot: &mut Option<Supervisor>) {
        self.begin_run().await;
        if let Some(sup) = slot.take() {
            let _ = sup.stop_tx.send(true);
            let mut task = sup.task;
            if tokio::time::timeout(Duration::from_secs(15), &mut task).await.is_err() {
                task.abort();
                crate::log_w!("[VK-туннель] Надзор не завершился за 15 с — прерван принудительно");
            }
        }
        let owned_ip = {
            let rt = self.rt.read().await;
            if rt.tun_configured { rt.tunnel_ip } else { None }
        };
        if owned_ip.is_some() { let _ = run_sh_checked(&tun_teardown_script_for_ip(owned_ip)).await; }
        let _ = tokio::fs::remove_file(PID_FILE).await;
        let mut rt = self.rt.write().await;
        rt.pid = None;
        rt.connected_streams = 0;
        rt.tunnel_ip = None;
        rt.tun_attached = false;
        rt.tun_configured = false;
        rt.phase = VkTurnPhase::Disabled;
    }

    async fn supervise(self: Arc<Self>, cfg: VkTurnConfig, mut stop_rx: watch::Receiver<bool>, state: crate::AppState) {
        let mut backoff = Duration::from_secs(5);
        let mut last_notice: Option<VkTurnPhase> = None;
        let _ = tokio::fs::create_dir_all(WORK_DIR).await;

        // TUN живёт, пока жив надзор: перезапуски клиента получают тот же интерфейс.
        let tun = loop {
            match TunHandle::create() {
                Ok(t) => break t,
                Err(e) => {
                    self.set_error(VkTurnPhase::Crashed, e).await;
                    if !sleep_or_stop(&mut stop_rx, backoff).await {
                        return;
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(120));
                }
            }
        };
        let tun = Arc::new(tun);
        let mut auto_call_ids: Vec<String> = Vec::new();

        loop {
            if *stop_rx.borrow() {
                break;
            }
            // Перед каждым запуском: XKeen/Zapret могли пересоздать свои цепочки и стереть исключения.
            let known_turn = self.rt.read().await.turn_ips.clone();
            apply_exemptions(&cfg.peer, &known_turn).await;

            // Завершаем прошлые авто-звонки, если были
            if let Some(tok) = selected_vk_token(&cfg) {
                finish_vk_calls_batch(&state.http, tok, &auto_call_ids).await;
                auto_call_ids.clear();
            }

            // Получаем хеши звонков: автоматически через calls.start (как в focsq) или из конфига
            let hashes = if let Some(tok) = selected_vk_token(&cfg) {
                if !tok.trim().is_empty() {
                    let needed = call_count_for_workers(effective_workers(&cfg));
                    let mut created_ids = Vec::new();
                    let mut created_hashes = Vec::new();
                    let mut last_err = None;

                    for i in 0..needed {
                        if i > 0 {
                            tokio::time::sleep(Duration::from_millis(150)).await;
                        }
                        match start_vk_call(&state.http, tok).await {
                            Ok((cid, h)) => {
                                crate::log_i!("[VK-туннель] Авто-звонок {cid} (хеш: {h}) успешно создан через API calls.start");
                                created_ids.push(cid);
                                created_hashes.push(h);
                            }
                            Err((is_invalid, e)) => {
                                last_err = Some((is_invalid, e));
                                break;
                            }
                        }
                    }

                    if created_hashes.is_empty() {
                        let (is_invalid, err_msg) = last_err.unwrap_or((false, "Не удалось создать звонок".into()));
                        crate::log_w!("[VK-туннель] Ошибка создания авто-звонка через API: {err_msg}");
                        {
                            let mut rt = self.rt.write().await;
                            rt.phase = if is_invalid { VkTurnPhase::AuthFailed } else { VkTurnPhase::Backoff };
                            rt.last_error = Some(err_msg);
                            rt.error_source = Some(if is_invalid { "vk" } else { "network" });
                            rt.error_reason = Some(if is_invalid { "authorization_rejected" } else { "call_creation_temporary" });
                        }
                        if is_invalid {
                            crate::log_w!("[VK-туннель] Токен VK отозван или аккаунт заблокирован. Остановка надзора до повторного входа.");
                            break;
                        }
                        if !sleep_or_stop(&mut stop_rx, backoff).await {
                            break;
                        }
                        backoff = (backoff * 2).min(Duration::from_secs(120));
                        continue;
                    }
                    auto_call_ids = created_ids;
                    created_hashes
                } else {
                    call_hashes(&cfg)
                }
            } else {
                call_hashes(&cfg)
            };

            let run_generation = self.begin_run().await;
            let uds_name = new_tun_uds_name();
            let mut cmd = tokio::process::Command::new(&self.core_path);
            cmd.args(build_client_args_for_uds(&cfg, &hashes, &uds_name))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            for (k, v) in client_env(&cfg) {
                cmd.env(k, v);
            }
            if std::path::Path::new(WORK_DIR).is_dir() {
                cmd.current_dir(WORK_DIR);
            }
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    self.set_error(VkTurnPhase::Crashed, format!("Не удалось запустить клиент: {e}")).await;
                    if !sleep_or_stop(&mut stop_rx, backoff).await {
                        break;
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(120));
                    continue;
                }
            };
            let pid = child.id();
            if let Some(p) = pid {
                let _ = tokio::fs::write(PID_FILE, p.to_string()).await;
            }
            {
                let mut rt = self.rt.write().await;
                rt.phase = VkTurnPhase::Starting;
                rt.pid = pid;
                rt.connected_streams = 0;
                rt.tun_attached = false;
                rt.started_at = Some(Instant::now());
                rt.zero_streams_since = Some(Instant::now());
                rt.getconf_silent = 0;
            }
            crate::log_i!(
                "[VK-туннель] Клиент csqtt запущен (pid {:?}, ссылок {}, потоков {})",
                pid,
                hashes.len(),
                effective_workers(&cfg)
            );

            // stdout (события, CAPTCHA_SOLVE) и stderr (лог) → разбор; действия — в канал надзора.
            let (eff_tx, mut eff_rx) = mpsc::unbounded_channel::<LineEffect>();
            let mut log_tasks = ClientLogTasks(Vec::new());
            for stream in [
                child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
                child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            ]
            .into_iter()
            .flatten()
            {
                log_tasks.0.push(self.spawn_log_reader(stream, eff_tx.clone()));
            }
            drop(eff_tx);
            let mut stdin = child.stdin.take();
            let mut fd_handoff: Option<FdHandoff> = None;
            let mut handoff_recovery = HandoffRecovery::new();

            // ждём: выход процесса / команду стоп / «висим без соединений» дольше допустимого
            let started = Instant::now();
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            let mut handoff_tick = tokio::time::interval(Duration::from_secs(1));
            let mut ticks: u32 = 0;
            let mut last_tun_rx = 0u64;
            let mut last_tun_tx = 0u64;
            let mut blackhole_ticks: u32 = 0;
            let mut exempted_turn = known_turn;
            let exit_reason = loop {
                tokio::select! {
                    st = child.wait() => break format!("клиент завершился: {:?}", st.ok()),
                    _ = stop_rx.changed() => {
                        eff_rx.close();
                        log_tasks.stop().await;
                        if let Some(handoff) = fd_handoff.take() { handoff.stop().await; }
                        // Завершаем звонки в VK API параллельно
                        if let Some(tok) = selected_vk_token(&cfg) {
                            finish_vk_calls_batch(&state.http, tok, &auto_call_ids).await;
                        }
                        // Мягкая остановка: клиент закрывает сессии, сервер сразу освобождает слоты.
                        if let Some(mut si) = stdin.take() {
                            let _ = si.write_all(b"STOP\n").await;
                            let _ = si.flush().await;
                        }
                        if tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_err() {
                            let _ = child.kill().await;
                        }
                        return;
                    }
                    Some(eff) = eff_rx.recv() => match eff {
                        LineEffect::CaptchaSolve => {
                            if let Some(si) = stdin.as_mut() {
                                let _ = si.write_all(CAPTCHA_NO_WEBVIEW_REPLY.as_bytes()).await;
                                let _ = si.flush().await;
                            }
                        }
                        LineEffect::TunConfig { ip, dns } => {
                            if let Err(error) = apply_tun_config(&self.rt, ip, dns, |script| async move { run_sh_checked(&script).await }).await {
                                self.set_error(VkTurnPhase::Crashed, error).await;
                                let _ = child.kill().await;
                                break "не удалось настроить интерфейс или маршруты туннеля".to_string();
                            }
                            handoff_recovery.configured = true;
                            if self.drive_handoff(&tun, run_generation, &uds_name, &mut fd_handoff, &mut handoff_recovery).await {
                                let _ = child.kill().await;
                                break "не удалось передать TUN после нескольких попыток".to_string();
                            }
                        }
                        LineEffect::None => {}
                    },
                    _ = handoff_tick.tick() => {
                        if self.drive_handoff(&tun, run_generation, &uds_name, &mut fd_handoff, &mut handoff_recovery).await {
                            let _ = child.kill().await;
                            break "не удалось передать TUN после нескольких попыток".to_string();
                        }
                    }
                    _ = tick.tick() => {
                        ticks = ticks.wrapping_add(1);
                        let rt = self.rt.read().await;
                        let turn_now = rt.turn_ips.clone();
                        let is_connected = rt.phase == VkTurnPhase::Connected && rt.tun_attached;
                        let stalled = rt.connected_streams == 0
                            && rt.zero_streams_since.map(|t| t.elapsed() > stall_limit(rt.phase)).unwrap_or(false);
                        drop(rt);

                        // раз в минуту (или при новых адресах TURN) восстанавливаем исключения
                        if ticks % 6 == 0 || turn_now != exempted_turn {
                            apply_exemptions(&cfg.peer, &turn_now).await;
                            exempted_turn = turn_now;
                        }

                        // Zero-CPU Blackhole Detector:
                        // Каждые 30 секунд (3 тика) пассивно проверяем дельту счетчиков csq0 из /proc/net/dev.
                        // Если клиенты шлют трафик (TX delta > 30 КБ), а ответа нет вообще (RX delta == 0)
                        // в течение 60 секунд подряд (2 замера) — сессия TURN заблокирована/сброшена:
                        // производим мягкий перезапуск для восстановления связи.
                        if is_connected && ticks % 3 == 0 {
                            let (cur_rx, cur_tx) = read_tun_interface_bytes(TUN_IFACE);
                            if last_tun_rx > 0 || last_tun_tx > 0 {
                                let tx_delta = cur_tx.saturating_sub(last_tun_tx);
                                let rx_delta = cur_rx.saturating_sub(last_tun_rx);
                                if tx_delta > 30_000 && rx_delta == 0 {
                                    blackhole_ticks += 1;
                                    if blackhole_ticks >= 2 {
                                        crate::log_w!(
                                            "[VK-туннель] Blackhole Detector: TX delta {} байт, RX delta 0 байт за 60 сек — сессия сброшена, мягкий перезапуск",
                                            tx_delta
                                        );
                                        let _ = child.kill().await;
                                        break "сессия TURN заблокирована (нет входящих пакетов при активном TX)".to_string();
                                    }
                                } else {
                                    blackhole_ticks = 0;
                                }
                            }
                            last_tun_rx = cur_rx;
                            last_tun_tx = cur_tx;
                        }

                        if stalled {
                            let _ = child.kill().await;
                            break "нет ни одного соединения дольше допустимого".to_string();
                        }
                    }
                }
            };

            eff_rx.close();
            log_tasks.stop().await;
            self.begin_run().await;
            if let Some(handoff) = fd_handoff.take() { handoff.stop().await; }

            let owned_ip = {
                let rt = self.rt.read().await;
                if rt.tun_configured { rt.tunnel_ip } else { None }
            };
            if owned_ip.is_some() {
                let _ = run_sh_checked(&tun_teardown_script_for_ip(owned_ip)).await;
            }

            let phase = {
                let mut rt = self.rt.write().await;
                rt.restarts += 1;
                rt.pid = None;
                rt.connected_streams = 0;
                rt.tun_attached = false;
                rt.tun_configured = false;
                rt.tunnel_ip = None;
                rt.phase
            };
            crate::log_w!("[VK-туннель] Перезапуск: {exit_reason}, состояние {phase:?}");

            let d = decide_after_exit(phase, started.elapsed(), &mut backoff, &mut last_notice);
            if let Some(text) = d.notice {
                notify_user(&state, text).await;
            }
            {
                let mut rt = self.rt.write().await;
                if !keeps_phase_during_backoff(rt.phase) {
                    rt.phase = VkTurnPhase::Backoff;
                }
            }
            if !sleep_or_stop(&mut stop_rx, d.wait).await {
                break;
            }
        }
    }

    pub(super) fn spawn_log_reader(
        self: &Arc<Self>,
        stream: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
        eff_tx: mpsc::UnboundedSender<LineEffect>,
    ) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut rt = me.rt.write().await;
                if eff_tx.is_closed() { break; }
                let eff = apply_log_line(&mut *rt, &line, Instant::now());
                drop(rt);
                if eff != LineEffect::None { let _ = eff_tx.send(eff); }
            }
        })
    }

    async fn drive_handoff(
        self: &Arc<Self>,
        tun: &Arc<TunHandle>,
        generation: u64,
        uds_name: &str,
        task: &mut Option<FdHandoff>,
        recovery: &mut HandoffRecovery,
    ) -> bool {
        if task.as_ref().is_some_and(|handoff| handoff.task.is_finished()) {
            if let Some(finished) = task.take() { finished.stop().await; }
            if self.rt.read().await.tun_attached {
                recovery.failures = 0;
            } else {
                recovery.failed();
            }
        }
        let attached = self.rt.read().await.tun_attached;
        match handoff_decision(recovery.configured, attached, task.is_some(), recovery.failures, Instant::now() >= recovery.retry_at) {
            HandoffDecision::Wait => false,
            HandoffDecision::Restart => true,
            HandoffDecision::Start => {
                *task = self.spawn_fd_handoff(tun, generation, uds_name.to_string());
                if task.is_none() { recovery.failed(); }
                recovery.failures >= 3 && task.is_none()
            }
        }
    }

    /// Передаёт клиенту копию дескриптора TUN в фоне: надзор продолжает читать вывод клиента.
    pub(super) async fn begin_run(&self) -> u64 {
        let mut rt = self.rt.write().await;
        rt.run_generation = rt.run_generation.wrapping_add(1);
        rt.run_generation
    }

    fn spawn_fd_handoff(self: &Arc<Self>, tun: &Arc<TunHandle>, generation: u64, uds_name: String) -> Option<FdHandoff> {
        let copy = match tun.duplicate() {
            Ok(c) => c,
            Err(e) => {
                crate::log_w!("[VK-туннель] {e}");
                return None;
            }
        };
        let me = self.clone();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let task = tokio::spawn(async move {
            let res = tokio::task::spawn_blocking(move || copy.send_to_client_blocking(&uds_name, &worker_cancel))
                .await
                .unwrap_or_else(|e| Err(format!("задача передачи TUN: {e}")));
            me.publish_handoff_result(generation, res).await;
        });
        Some(FdHandoff { cancel, task })
    }

    pub(super) async fn publish_handoff_result(&self, generation: u64, res: Result<(), String>) {
            let mut rt = self.rt.write().await;
            if rt.run_generation != generation { return; }
            match res {
                Ok(()) => {
                    rt.tun_attached = true;
                    push_log(&mut rt, format!("[ТУННЕЛЬ] Интерфейс {TUN_IFACE} передан клиенту"));
                }
                Err(e) => {
                    rt.last_error = Some(e.clone());
                    push_log(&mut rt, format!("[ОШИБКА] {e}"));
                }
            }
    }

    async fn set_error(&self, phase: VkTurnPhase, msg: String) {
        let mut rt = self.rt.write().await;
        rt.phase = phase;
        rt.last_error = Some(msg);
        rt.error_source = None;
        rt.error_reason = None;
    }
}

async fn sleep_or_stop(stop_rx: &mut watch::Receiver<bool>, d: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => !*stop_rx.borrow(),
        _ = stop_rx.changed() => false,
    }
}

async fn kill_stale_from_pidfile() {
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("killall").arg("csqtt-client").status().await;
    }
    let Ok(s) = tokio::fs::read_to_string(PID_FILE).await else { return };
    let Ok(pid) = s.trim().parse::<u32>() else { return };
    if let Ok(cmd) = tokio::fs::read(format!("/proc/{pid}/cmdline")).await {
        if String::from_utf8_lossy(&cmd).contains("csqtt-client") {
            let _ = tokio::process::Command::new("kill").arg("-9").arg(pid.to_string()).status().await;
        }
    }
    let _ = tokio::fs::remove_file(PID_FILE).await;
}

pub fn spawn_vkturn_notification_task<F>(timeout: Duration, fut: F) -> tokio::task::JoinHandle<bool>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        tokio::time::timeout(timeout, fut).await.is_ok()
    })
}

async fn notify_user(state: &crate::AppState, text: &str) {
    let cfg = state.config.read().await;
    if !cfg.notifications.telegram_enabled
        || cfg.notifications.telegram_bot_token.is_empty()
        || cfg.notifications.telegram_chat_id.is_empty()
    {
        return;
    }
    let proxy_url = cfg.mihomo_proxy_url();
    let http = state.http.clone();
    let token = cfg.notifications.telegram_bot_token.clone();
    let chat_id = cfg.notifications.telegram_chat_id.clone();
    let msg = text.to_string();
    drop(cfg);

    let _ = spawn_vkturn_notification_task(Duration::from_secs(10), async move {
        let _ = crate::notifications::send_telegram_with_proxy(
            &http,
            Some(&proxy_url),
            &token,
            &chat_id,
            &msg,
        )
        .await;
    });
}

// ---------- Установка ядра ----------

/// Целевая архитектура для validate_elf_header по строке detect_mihomo_arch().
pub fn elf_target_for_arch(arch: &str) -> &'static str {
    let a = arch.to_ascii_lowercase();
    if a.starts_with("mipsle") || a.starts_with("mipsel") {
        "mipsel"
    } else if a.starts_with("mips") {
        "mips"
    } else if a.contains("arm64") || a.contains("aarch64") {
        "aarch64"
    } else if a.starts_with("arm") {
        "arm"
    } else {
        "x86_64"
    }
}

pub async fn install_core(state: &crate::AppState) -> Result<String, String> {
    let arch = crate::updater::detect_mihomo_arch().await;
    let asset = core_asset_for_arch(arch).ok_or(format!(
        "VK-туннель на ядре csqtt пока работает только на роутерах с процессором ARM64 (у этого роутера: {arch})"
    ))?;
    let expected = CORE_SHA256
        .iter()
        .find(|(a, _)| *a == asset)
        .map(|(_, h)| *h)
        .ok_or("Для этой архитектуры нет проверенного хеша ядра")?;
    let url = format!("{CORE_RELEASES}/{CORE_TAG}/{asset}");
    let tmp = format!("{CORE_PATH}.tmp");

    let cfg = state.config.read().await.clone();
    let proxy_url = cfg.mihomo_proxy_url();
    let install = state.vkturn.install.clone();
    let progress = move |done: u64, total: Option<u64>| {
        if let Ok(mut g) = install.lock() {
            g.downloaded = done;
            g.total = total;
        }
    };
    let bytes = crate::updater::download_file_verified_with_progress(
        &state.http,
        &url,
        Some(expected),
        &proxy_url,
        crate::updater::DownloadLimits::default(),
        &progress,
    )
    .await?;

    if let Some(parent) = std::path::Path::new(CORE_PATH).parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    if let Err(e) = tokio::fs::write(&tmp, &bytes).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Ошибка записи временного файла: {e}"));
    }

    if let Err(e) = crate::updater::validate_elf_header(&bytes, elf_target_for_arch(arch)) {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Некорректный ELF-заголовок ядра: {e}"));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = tokio::fs::metadata(&tmp).await {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = tokio::fs::set_permissions(&tmp, perms).await;
        }

        // Пробный запуск без пароля: клиент сразу выходит с ошибкой «нужен пароль». Важно лишь,
        // что процесс запустился на этом процессоре (не убит сигналом).
        let test_run = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new(&tmp)
                .env_remove("CSQTT_PASSWORD")
                .stdin(Stdio::null())
                .output(),
        )
        .await;

        match test_run {
            Ok(Ok(out)) => {
                if out.status.code().is_none() {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err("Ядро не запускается на этом процессоре (завершено сигналом)".into());
                }
            }
            Ok(Err(e)) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(format!("Ядро не запускается на этом процессоре ({e})"));
            }
            Err(_) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err("Таймаут при пробном запуске ядра".into());
            }
        }
    }

    if let Err(e) = tokio::fs::rename(&tmp, CORE_PATH).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Ошибка перемещения бинарника в {CORE_PATH}: {e}"));
    }
    // Ядро прошлой версии панели больше не используется.
    let _ = tokio::fs::remove_file(LEGACY_CORE_PATH).await;
    let _ = tokio::fs::remove_dir_all("/opt/var/lib/wdtt").await;

    Ok(format!("Ядро VK-туннеля csqtt ({asset}) успешно установлено"))
}

// ---------- Тесты ----------

