use crate::gallery::{Entry as GalleryEntry, Viewer as GalleryViewer};
use crate::icons::{self, Icon};
use crate::{
    api::{Metadata, Query},
    assets::{Loaded, Source},
    cloud::{self, Job as CloudJob},
    settings::{Settings, Theme},
    system::{self, Appearance},
    worker::{DiskJob, Event, NetJob, Workers},
};
use eframe::egui::{self, Color32, RichText, TextureHandle, TextureOptions};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    time::{Duration, Instant},
};

const HISTORY_LIMIT: usize = 48;
const MEMORY_LIMIT: usize = 96 * 1024 * 1024;
#[derive(Clone)]
struct Item {
    meta: Metadata,
    source: Source,
    dimensions: [u32; 2],
    thumb: TextureHandle,
    saved: bool,
}
#[derive(PartialEq, Eq)]
enum View {
    Discover,
    Collection,
    Favorites,
    Albums,
}

pub struct GalleryApp {
    base: PathBuf,
    lightbox: Option<GalleryViewer>,
    gallery_epoch: u64,
    gallery_context: egui::Context,
    gallery_tile_width: f32,
    gallery_search: String,
    updates: crate::updater::Updater,
    update_ready: Option<crate::updater::Ready>,
    update_busy: bool,
    update_status: String,
    update_restart: bool,
    install_after_exit: bool,
    settings: Settings,
    workers: Workers,
    appearance: Appearance,
    last_os_check: Instant,
    os_pending: bool,
    style_key: Option<(bool, [u8; 3])>,
    tags: Vec<String>,
    source_draft: crate::sources::SourceProfile,
    source_status: String,
    source_busy: bool,
    tag_search: String,
    tags_loading: bool,
    view: View,
    history: Vec<Item>,
    selected: Option<usize>,
    preview: Option<TextureHandle>,
    zoom: f32,
    epoch: u64,
    net_busy: bool,
    waiting: bool,
    ready: Option<Box<Loaded>>,
    open_epoch: u64,
    opening: bool,
    saving: HashSet<String>,
    saved_sources: HashMap<String,Vec<PathBuf>>,
    library: Vec<PathBuf>,
    library_epoch: u64,
    scanning: bool,
    scan_due: bool,
    library_search: String,
    thumbs: HashMap<PathBuf, TextureHandle>,
    thumb_order: VecDeque<PathBuf>,
    thumb_pending: HashSet<PathBuf>,
    thumb_failed: HashSet<PathBuf>,
    remote_thumbs: HashMap<String, TextureHandle>,
    thumb_warmup: VecDeque<(Metadata,Option<PathBuf>)>,
    remote_pending: HashSet<String>,
    remote_failed: HashSet<String>,
    settings_open: bool,
    settings_tab: u8,
    filters_open: bool,
    account_open: bool,
    cloud: cloud::Worker,
    cloud_data: cloud::Data,
    cloud_user: Option<String>,
    cloud_busy: bool,
    cloud_last: Instant,
    cloud_status: String,
    cloud_error: bool,
    login_name: String,
    login_password: String,
    login_register: bool,
    pending_appearance: Option<cloud::Appearance>,
    appearance_retry: Instant,
    album_name: String,
    album_selected: Option<String>,
    album_rename: String,
    album_confirm_delete: bool,
    dirty: Option<Instant>,
    status: String,
    error: bool,
    started: Instant,
    smoke: bool,
    smoke_phase: u8,
    smoke_path: Option<PathBuf>,
    closing: bool,
    capture_after: Instant,
}

impl GalleryApp {
    pub fn new(cc: &eframe::CreationContext<'_>, base: PathBuf, smoke: bool) -> Self {
        let (settings, warning) = Settings::load(&base);
        let workers = Workers::start_with_cache(cc.egui_ctx.clone(),Some(base.join("cache/thumbnails")));
        if !settings.use_custom_url {let _ = workers.net.try_send(NetJob::Tags);}
        let _ = workers.disk.try_send(DiskJob::Appearance);
        let cloud = cloud::Worker::start(cc.egui_ctx.clone());
        let _ = cloud
            .tx
            .try_send(CloudJob::Resume(settings.sync_server.clone()));
        let updates = crate::updater::Updater::start(cc.egui_ctx.clone());
        let mut app = Self {
            base,
            gallery_context: cc.egui_ctx.clone(),
            lightbox: None,
            gallery_epoch: 0,
            gallery_tile_width: 220.0,
            gallery_search: String::new(),
            updates,
            update_ready: None,
            update_busy: false,
            update_status: "Обновления из GitHub Releases".into(),
            update_restart: false,
            install_after_exit: false,
            settings: settings.clone(),
            workers,
            appearance: Appearance {
                light: false,
                accent: None,
            },
            last_os_check: Instant::now(),
            os_pending: true,
            style_key: None,
            source_draft: Default::default(),
            source_status: String::new(),
            source_busy: false,
            tags: vec!["waifu".into()],
            tag_search: String::new(),
            tags_loading: !settings.use_custom_url,
            view: View::Discover,
            history: vec![],
            selected: None,
            preview: None,
            zoom: 1.0,
            epoch: 0,
            net_busy: false,
            waiting: false,
            ready: None,
            open_epoch: 0,
            opening: false,
            saving: HashSet::new(),
            saved_sources: HashMap::new(),
            library: vec![],
            library_epoch: 0,
            scanning: false,
            scan_due: false,
            library_search: String::new(),
            thumbs: HashMap::new(),
            thumb_order: VecDeque::new(),
            thumb_pending: HashSet::new(),
            thumb_failed: HashSet::new(),
            remote_thumbs: HashMap::new(),
            thumb_warmup: VecDeque::new(),
            remote_pending: HashSet::new(),
            remote_failed: HashSet::new(),
            settings_open: false,
            settings_tab: 0,
            filters_open: false,
            account_open: false,
            cloud,
            cloud_data: cloud::Data::default(),
            cloud_user: None,
            cloud_busy: true,
            cloud_last: Instant::now(),
            cloud_status: "Проверяем сохранённый вход…".into(),
            cloud_error: false,
            login_name: String::new(),
            login_password: String::new(),
            login_register: false,
            pending_appearance: None,
            appearance_retry: Instant::now(),
            album_name: String::new(),
            album_selected: None,
            album_rename: String::new(),
            album_confirm_delete: false,
            dirty: None,
            status: warning
                .clone()
                .unwrap_or("Нажми «Новая картинка», чтобы начать.".into()),
            error: warning.is_some(),
            started: Instant::now(),
            smoke,
            smoke_phase: 0,
            smoke_path: None,
            closing: false,
            capture_after: Instant::now(),
        };
        if app.settings.use_custom_url {app.refresh_source_tags();}
        app.scan();
        app.apply_style(&cc.egui_ctx);
        app
    }
    fn query(&self) -> Query {
        Query {
            mode: self.settings.mode.clone(),
            tag: self.settings.category.clone(),
            custom_url: if self.settings.use_custom_url {
                Some(self.settings.custom_url.trim().into())
            } else {
                None
            },
        }
    }
    fn accent(&self) -> [u8; 3] {
        palette(&self.settings, self.appearance).1
    }
    fn ac_text(&self) -> Color32 {
        let [r, g, b] = system::readable_accent(self.accent(), self.dark());
        Color32::from_rgb(r, g, b)
    }
    fn dark(&self) -> bool {
        palette(&self.settings, self.appearance).0
    }
    fn background(&self) -> Color32 {
        if self.dark() {
            Color32::from_rgb(14, 16, 23)
        } else {
            Color32::from_rgb(244, 245, 250)
        }
    }
    fn panel(&self) -> Color32 {
        if self.dark() {
            Color32::from_rgb(20, 23, 32)
        } else {
            Color32::WHITE
        }
    }
    fn card(&self) -> Color32 {
        if self.dark() {
            Color32::from_rgb(27, 31, 43)
        } else {
            Color32::from_rgb(236, 238, 246)
        }
    }
    fn muted(&self) -> Color32 {
        if self.dark() {
            Color32::from_rgb(153, 162, 184)
        } else {
            Color32::from_rgb(93, 103, 125)
        }
    }
    fn ac(&self) -> Color32 {
        let [r, g, b] = self.accent();
        Color32::from_rgb(r, g, b)
    }
    fn message(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.error = error;
    }
    fn changed(&mut self) {
        self.dirty = Some(Instant::now());
    }
    fn apply_style(&mut self, ctx: &egui::Context) {
        let key = (self.dark(), self.accent());
        if self.style_key == Some(key) {
            return;
        }
        let mut visuals = if key.0 {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = self.panel();
        visuals.window_fill = self.panel();
        visuals.extreme_bg_color = self.background();
        visuals.faint_bg_color = self.card();
        visuals.selection.bg_fill = self.ac();
        let [r, g, b] = system::contrast_text(self.accent());
        visuals.selection.stroke = egui::Stroke::new(1.0, Color32::from_rgb(r, g, b));
        visuals.widgets.inactive.bg_fill = self.card();
        visuals.widgets.hovered.bg_fill = self.ac().gamma_multiply(0.35);
        visuals.widgets.active.bg_fill = self.ac().gamma_multiply(0.5);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(9);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(9);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(9);
        visuals.window_corner_radius = egui::CornerRadius::same(16);
        ctx.set_visuals(visuals);
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if key.0 {
            egui::SystemTheme::Dark
        } else {
            egui::SystemTheme::Light
        }));
        ctx.style_mut(|s| {
            s.spacing.item_spacing = egui::vec2(10.0, 10.0);
            s.spacing.button_padding = egui::vec2(14.0, 10.0);
            s.text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            s.text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
            s.text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
        });
        self.style_key = Some(key);
    }
    fn invalidate(&mut self) {
        self.epoch += 1;
        self.ready = None;
        self.waiting = false;
        self.changed();
        self.scan();
        self.message("Фильтры обновлены. Нажми «Следующая».", false);
    }
    fn fetch(&mut self) {
        if self.net_busy {
            return;
        }
        let query = self.query();
        if query.custom_url.as_ref().is_some_and(|s| s.is_empty()) {
            self.waiting = false;
            self.message("Укажи URL API в настройках.", true);
            return;
        }
        if self
            .workers
            .net
            .try_send(NetJob::Image(self.epoch, query))
            .is_ok()
        {
            self.net_busy = true;
        } else {
            self.waiting = false;
            self.message("Очередь загрузки занята. Попробуй ещё раз.", true);
        }
    }
    fn next(&mut self, ctx: &egui::Context) {
        if self.waiting {
            return;
        }
        self.view = View::Discover;
        if let Some(image) = self.ready.take() {
            self.present(ctx, *image);
            self.prefetch();
        } else {
            self.waiting = true;
            self.fetch();
            self.message("Ищем следующую находку…", false);
        }
    }
    fn prefetch(&mut self) {
        if self.settings.prefetch && self.ready.is_none() {
            self.fetch();
        }
    }
    fn present(&mut self, ctx: &egui::Context, loaded: Loaded) {
        self.open_epoch += 1;
        self.opening = false;
        let key = loaded.meta.key.clone();
        self.history.retain(|item| item.meta.key != key);
        let preview = ctx.load_texture("current-preview", loaded.preview, TextureOptions::LINEAR);
        let thumb = ctx.load_texture(
            format!("thumb-{key}"),
            loaded.thumbnail,
            TextureOptions::LINEAR,
        );
        let saved = matches!(loaded.source, Source::Disk(_)) || self.saved_sources.contains_key(&loaded.meta.key);
        self.history.insert(
            0,
            Item {
                meta: loaded.meta,
                source: loaded.source,
                dimensions: loaded.dimensions,
                thumb,
                saved,
            },
        );
        self.selected = Some(0);
        self.preview = Some(preview);
        self.zoom = 1.0;
        self.waiting = false;
        trim_history(&mut self.history);
        self.message("Готово. Сохрани то, что нравится.", false);
    }
    fn select(&mut self, index: usize) {
        if index >= self.history.len() {
            return;
        }
        self.open_epoch += 1;
        let item = &self.history[index];
        if self
            .workers
            .disk
            .try_send(DiskJob::Open(
                self.open_epoch,
                item.source.clone(),
                item.meta.clone(),
            ))
            .is_ok()
        {
            self.selected = Some(index);
            self.opening = true;
            self.preview = None;
            self.zoom = 1.0;
        } else {
            self.message("Завершаем текущую операцию. Попробуй ещё раз.", true);
        }
    }
    fn open_local(&mut self, path: PathBuf) {
        let mut entries: Vec<_> = self
            .library
            .iter()
            .cloned()
            .map(GalleryEntry::Local)
            .collect();
        let index = entries
            .iter()
            .position(|e| e.key() == path.to_string_lossy())
            .unwrap_or_else(|| {
                entries.push(GalleryEntry::Local(path));
                entries.len() - 1
            });
        self.open_gallery(entries, index);
    }
    fn save_current(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let item = &self.history[index];
        if item.saved || self.saved_sources.contains_key(&item.meta.key) || self.saving.contains(&item.meta.key) {
            return;
        }
        let folder = self
            .settings
            .save_folder(&self.base, &item.meta.mode, &item.meta.tag);
        let key = item.meta.key.clone();
        if self
            .workers
            .disk
            .try_send(DiskJob::SaveWithMetadata(
                item.meta.clone(),
                item.source.clone(),
                folder,
            ))
            .is_ok()
        {
            self.saving.insert(key);
            self.message("Сохраняем полный размер…", false);
        } else {
            self.message("Очередь сохранения занята.", true);
        }
    }
    fn scan(&mut self) {
        self.library_epoch += 1;
        self.scan_due = true;
    }
    fn events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.workers.events.try_recv() {
            match event {
                Event::GalleryPrefetch(session,key,result)=>self.gallery_prefetch_result(session,key,result),
                Event::SavedIndex(epoch,index) => {if epoch==self.library_epoch{self.saved_sources=index;self.queue_favorite_thumbnails();}},
                Event::Gallery(epoch, result) => {
                    self.gallery_result(ctx, epoch, result);
                }
                Event::RemoteThumb(key, result) => {
                    self.remote_pending.remove(&key);
                    match result {
                        Ok(image) => {
                            if self.remote_thumbs.len() >= 128 {
                                if let Some(old) = self.remote_thumbs.keys().next().cloned() {
                                    self.remote_thumbs.remove(&old);
                                }
                            }
                            self.remote_thumbs.insert(
                                key.clone(),
                                ctx.load_texture(
                                    format!("cloud-{key}"),
                                    image,
                                    TextureOptions::LINEAR,
                                ),
                            );
                        }
                        Err(_) => {
                            self.remote_failed.insert(key);
                        }
                    }
                }
                Event::SourceTags(url, mode, result) => {
                    if self.settings.use_custom_url && self.settings.custom_url==url && self.settings.mode==mode {
                        self.source_busy=false;self.tags_loading=false;
                        match result {Ok(tags)=>{let template=crate::sources::infer_template(&url,&tags);self.settings.custom_url=template.clone();self.invalidate();self.source_status=format!("Найдено категорий: {}",tags.len());if let Some(p)=self.settings.sources.iter_mut().find(|p|p.url==url){p.categories=tags.clone();p.categories_mode=mode.clone();p.url=template;}self.set_source_tags(tags);self.changed();},Err(e)=>self.source_status=e}
                    }
                },
                Event::Tags(result) => {
                    if self.settings.use_custom_url {continue;}
                    self.tags_loading = false;
                    match result {
                        Ok(tags) => {
                            self.tags = tags;
                            self.tags.sort_by(|a, b| {
                                (a.as_str() != "waifu", a).cmp(&(b.as_str() != "waifu", b))
                            });
                            if !self.tags.contains(&self.settings.category) {
                                self.settings.category = if self.tags.iter().any(|s| s == "waifu") {
                                    "waifu".into()
                                } else {
                                    self.tags[0].clone()
                                };
                                self.invalidate();
                            }
                        }
                        Err(e) => {
                            self.message(format!("Теги недоступны. Доступен waifu. {e}"), true)
                        }
                    }
                }
                Event::Image(epoch, result) => {
                    self.net_busy = false;
                    if epoch != self.epoch {
                        if self.waiting {
                            self.fetch();
                        }
                        continue;
                    }
                    match result {
                        Ok(loaded) => {
                            if self.waiting {
                                self.present(ctx, *loaded);
                                self.prefetch();
                            } else if self.settings.prefetch {
                                self.ready = Some(loaded);
                            }
                        }
                        Err(e) => {
                            let was_waiting = self.waiting;
                            self.waiting = false;
                            if was_waiting {
                                self.message(e, true);
                            }
                        }
                    }
                }
                Event::Open(epoch, result) => {
                    if epoch != self.open_epoch {
                        continue;
                    }
                    self.opening = false;
                    match result {
                        Ok(loaded) => {
                            if let Some(index) = self.selected {
                                self.history[index].dimensions = loaded.dimensions;
                                self.preview = Some(ctx.load_texture(
                                    "current-preview",
                                    loaded.preview,
                                    TextureOptions::LINEAR,
                                ));
                            } else {
                                self.present(ctx, *loaded);
                            }
                        }
                        Err(e) => self.message(e, true),
                    }
                }
                Event::Saved(key, result) => {
                    self.saving.remove(&key);
                    match result {
                        Ok(path) => {
                            let paths=self.saved_sources.entry(key.clone()).or_default();if !paths.contains(&path){paths.push(path.clone());}
                            if let Some(g) = self.lightbox.as_mut() {
                                for (_, loaded) in &mut g.cache {
                                    if loaded.meta.key == key {
                                        loaded.source = Source::Disk(path.clone());
                                    }
                                }
                            }
                            for item in &mut self.history {
                                if item.meta.key == key {
                                    item.saved = true;
                                    item.source = Source::Disk(path.clone());
                                }
                            }
                            self.smoke_path = Some(path.clone());
                            self.message(
                                "Картинка сохранена. Она доступна в разделе «Сохранённое».",
                                false,
                            );
                            self.scan();
                        }
                        Err(e) => self.message(format!("Не удалось сохранить: {e}"), true),
                    }
                }
                Event::Scan(epoch, result) => {
                    self.scanning = false;
                    if epoch != self.library_epoch {
                        continue;
                    }
                    self.scanning = false;
                    match result {
                        Ok(paths) => {
                            self.library = paths;
                        }
                        Err(e) => self.message(format!("Коллекция: {e}"), true),
                    }
                }
                Event::Thumb(path, result) => {
                    self.thumb_pending.remove(&path);
                    match result {
                        Ok(color) => {
                            while self.thumbs.len() >= 128 {
                                if let Some(old) = self.thumb_order.pop_front() {
                                    self.thumbs.remove(&old);
                                } else {
                                    break;
                                }
                            }
                            self.thumb_order.push_back(path.clone());
                            self.thumbs.insert(
                                path.clone(),
                                ctx.load_texture(
                                    path.to_string_lossy(),
                                    color,
                                    TextureOptions::LINEAR,
                                ),
                            );
                        }
                        Err(_) => {
                            self.thumb_failed.insert(path);
                        }
                    }
                }
                Event::Settings(result) => {
                    if let Err(e) = result {
                        self.message(format!("Настройки не сохранены: {e}"), true);
                    }
                }
                Event::Appearance(value) => {
                    self.appearance = value;
                    self.os_pending = false;
                }
            }
        }
    }
    fn primary(&self, ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
        let [r, g, b] = system::contrast_text(self.accent());
        ui.add_enabled(
            enabled,
            icons::button(text, icons::for_label(text))
                .color(Color32::from_rgb(r, g, b))
                .strong()
                .fill(self.ac())
                .min_size(egui::vec2(176.0, 42.0)),
        )
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(206.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(self.panel()).inner_margin(18))
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.label(
                    RichText::new("waifu")
                        .size(31.0)
                        .strong()
                        .color(self.ac_text()),
                );
                ui.label(RichText::new("Твоя галерея").size(12.0).color(self.muted()));
                ui.add_space(30.0);
                for (view, label, hint) in [
                    (View::Discover, "Просмотр", "Новые картинки и история"),
                    (View::Collection, "Сохранённое", "Файлы на этом компьютере"),
                    (
                        View::Favorites,
                        "Избранное",
                        "Синхронизация между устройствами",
                    ),
                    (View::Albums, "Альбомы", "Твои подборки изображений"),
                ] {
                    let active = self.view == view;
                    if ui
                        .add_sized(
                            [ui.available_width(), 44.0],
                            icons::button(
                                label,
                                match view {
                                    View::Discover => Icon::Image,
                                    View::Collection => Icon::Collection,
                                    View::Favorites => Icon::Heart,
                                    View::Albums => Icon::Folder,
                                },
                            )
                            .strong()
                            .fill(if active {
                                self.ac().gamma_multiply(0.22)
                            } else {
                                Color32::TRANSPARENT
                            }),
                        )
                        .on_hover_text(hint)
                        .clicked()
                    {
                        self.view = view;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(
                        RichText::new("Изображения · Waifu.im")
                            .size(10.0)
                            .color(self.muted()),
                    );
                    ui.add_space(12.0);
                    if ui
                        .add_sized(
                            [ui.available_width(), 38.0],
                            icons::button("Настройки", Icon::Settings),
                        )
                        .clicked()
                    {
                        self.settings_open = true;
                    }
                    let label = self.cloud_user.as_deref().unwrap_or("Войти в аккаунт");
                    if ui
                        .add_sized(
                            [ui.available_width(), 42.0],
                            icons::button(label, Icon::User),
                        )
                        .clicked()
                    {
                        self.account_open = true;
                    }
                    if self.cloud_user.is_some() {
                        ui.label(
                            RichText::new(if self.cloud_error {
                                "Нет связи с сервером"
                            } else if self.pending_appearance.is_some() {
                                "Оформление ожидает отправки"
                            } else if self.cloud_busy {
                                "Синхронизация…"
                            } else {
                                "Аккаунт подключён"
                            })
                            .size(11.0)
                            .color(self.muted()),
                        );
                    }
                });
            });
    }
    fn header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(match self.view {
                        View::Discover => "Находи то, что нравится",
                        View::Collection => "На этом компьютере",
                        View::Favorites => "Всегда под рукой",
                        View::Albums => "Всё по своим местам",
                    })
                    .size(12.0)
                    .color(self.muted()),
                );
                ui.label(
                    RichText::new(match self.view {
                        View::Discover => "Просмотр",
                        View::Collection => "Сохранённое",
                        View::Favorites => "Избранное",
                        View::Albums => "Альбомы",
                    })
                    .size(29.0)
                    .strong(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.view == View::Discover {
                    if self
                        .primary(
                            ui,
                            if self.waiting {
                                "Загрузка…"
                            } else {
                                "Новая картинка"
                            },
                            !self.waiting && !self.opening,
                        )
                        .clicked()
                    {
                        self.next(ctx);
                    }
                    let (enabled, label) = self
                        .selected
                        .map(|i| {
                            let item = &self.history[i];
                            (
                                !item.saved && !self.saved_sources.contains_key(&item.meta.key) && !self.saving.contains(&item.meta.key),
                                if item.saved || self.saved_sources.contains_key(&item.meta.key) {
                                    "Сохранено"
                                } else if self.saving.contains(&item.meta.key) {
                                    "Сохраняем…"
                                } else {
                                    "Сохранить"
                                },
                            )
                        })
                        .unwrap_or((false, "Сохранить"));
                    if ui
                        .add_enabled(
                            enabled,
                            icons::button(label, icons::for_label(label))
                                .min_size(egui::vec2(130.0, 42.0)),
                        )
                        .clicked()
                    {
                        self.save_current();
                    }
                    let meta = self.selected.map(|i| self.history[i].meta.clone());
                    if let Some(meta) = meta {
                        let local =
                            !(meta.key.starts_with("http://") || meta.key.starts_with("https://"));
                        let path = match &self.history[self.selected.unwrap()].source {
                            Source::Disk(p) => Some(p.clone()),
                            _ => None,
                        };
                        let favorite = if local {
                            path.as_ref()
                                .is_some_and(|p| self.settings.local_favorites.contains(p))
                        } else {
                            self.photo_favorite(&meta.key)
                        };
                        if ui
                            .add_enabled(
                                local || !self.cloud_busy,
                                icons::button(
                                    if favorite {
                                        "В избранном"
                                    } else {
                                        "В избранное"
                                    },
                                    if favorite {
                                        Icon::HeartFilled
                                    } else {
                                        Icon::Heart
                                    },
                                )
                                .min_size(egui::vec2(110.0, 42.0)),
                            )
                            .clicked()
                        {
                            self.toggle_photo_favorite(meta,path);
                        }
                    }
                } else if self.view == View::Collection {
                    if ui.add(icons::button("Обновить", Icon::Refresh)).clicked() {
                        self.scan();
                    }
                } else if self.cloud_user.is_some()
                    && ui
                        .add_enabled(
                            !self.cloud_busy,
                            icons::button("Синхронизировать", Icon::Refresh),
                        )
                        .clicked()
                {
                    self.cloud_send(CloudJob::Refresh);
                }
            });
        });
        ui.add_space(14.0);
        if self.view == View::Discover {
            ui.horizontal(|ui| {
                if ui
                    .button(format!(
                        "Категория: {}…",
                        tag_label(&self.settings.category)
                    ))
                    .clicked()
                {
                    self.filters_open = true;
                }
                for (mode, label) in [("SFW", "Обычные"), ("NSFW", "18+")] {
                    if ui
                        .selectable_label(self.settings.mode == mode, label)
                        .clicked()
                        && self.settings.mode != mode
                    {
                        self.settings.mode = mode.into();
                        if self.settings.use_custom_url {self.refresh_source_tags();}
                        self.invalidate();
                    }
                }
                if self.settings.use_custom_url {
                    ui.small("Свой источник");
                }
            });
            ui.add_space(12.0);
        }
    }
    fn filters_window(&mut self, ctx: &egui::Context) {
        if !self.filters_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Выбрать категорию")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(470.0)
            .show(ctx, |ui| {
                ui.label("Выбери настроение для следующих изображений");
                ui.add(
                    egui::TextEdit::singleline(&mut self.tag_search)
                        .hint_text("Поиск категории…")
                        .desired_width(f32::INFINITY),
                );
                if self.tags_loading {
                    ui.spinner();
                }
                let search = self.tag_search.to_lowercase();
                let tags: Vec<_> = self
                    .tags
                    .iter()
                    .filter(|t| {
                        t.contains(&search) || tag_label(t).to_lowercase().contains(&search)
                    })
                    .cloned()
                    .collect();
                if tags.is_empty() {
                    ui.label("Ничего не найдено");
                }
                egui::ScrollArea::vertical()
                    .max_height(340.0)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for tag in tags {
                                if ui
                                    .selectable_label(
                                        self.settings.category == tag,
                                        tag_label(&tag),
                                    )
                                    .clicked()
                                {
                                    self.settings.category = tag;
                                    self.invalidate();
                                    self.filters_open = false;
                                }
                            }
                        });
                    });
            });
        self.filters_open &= open;
    }
    fn viewer(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let film_height = if self.history.is_empty() { 0.0 } else { 116.0 };
        let canvas_height = (ui.available_height() - film_height - 68.0).max(230.0);
        egui::Frame::new()
            .fill(self.panel())
            .corner_radius(18)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_min_height(canvas_height);
                if self.opening || (self.preview.is_none() && self.waiting) {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), canvas_height),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add_space(canvas_height * 0.4);
                            ui.spinner();
                            ui.label("Готовим изображение…");
                        },
                    );
                } else if let Some(texture) = &self.preview {
                    let available = egui::vec2(ui.available_width(), canvas_height);
                    let original = texture.size_vec2();
                    let fit = (available.x / original.x)
                        .min(available.y / original.y)
                        .min(1.0);
                    let size = original * fit * self.zoom;
                    egui::ScrollArea::both()
                        .id_salt("preview")
                        .max_height(canvas_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let width = ui.available_width().max(size.x);
                            ui.allocate_ui_with_layout(
                                egui::vec2(width, canvas_height.max(size.y)),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.add_space(((canvas_height - size.y) / 2.0).max(0.0));
                                    ui.add(
                                        egui::Image::new((texture.id(), size)).corner_radius(10),
                                    );
                                },
                            );
                        });
                } else {
                    let available = egui::vec2(ui.available_width(), canvas_height);
                    ui.allocate_ui_with_layout(
                        available,
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add_space((canvas_height * 0.22).max(16.0));
                            egui::Frame::new()
                                .fill(self.ac().gamma_multiply(0.15))
                                .corner_radius(24)
                                .inner_margin(22)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("W")
                                            .size(50.0)
                                            .strong()
                                            .color(self.ac_text()),
                                    );
                                });
                            ui.add_space(16.0);
                            ui.label(RichText::new("Место для вдохновения").size(25.0).strong());
                            ui.label(
                                RichText::new(
                                    "Нажми «Новая картинка» — мы найдём что-нибудь для тебя.",
                                )
                                .color(self.muted()),
                            );
                            ui.add_space(14.0);
                            if self
                                .primary(ui, "Найти первую картинку", !self.waiting)
                                .clicked()
                            {
                                self.next(&ctx);
                            }
                            ui.label(
                                RichText::new("Или просто нажми пробел.")
                                    .size(12.0)
                                    .color(self.muted()),
                            );
                        },
                    );
                }
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if let Some(index) = self.selected {
                let item = &self.history[index];
                ui.label(
                    RichText::new(format!("{} × {}", item.dimensions[0], item.dimensions[1]))
                        .strong(),
                );
                ui.label(
                    RichText::new(format!(
                        "{} · {}",
                        if item.meta.mode == "NSFW" {
                            "18+"
                        } else {
                            "Обычные"
                        },
                        tag_label(&item.meta.tag)
                    ))
                    .color(self.muted()),
                );
                if !item.meta.artist.is_empty() {
                    ui.label(RichText::new(&item.meta.artist).color(self.muted()));
                }
                if let Some(source) = &item.meta.source {
                    ui.hyperlink_to("Источник ↗", source);
                }
            } else {
                ui.label(
                    RichText::new("Оригинальное разрешение при сохранении")
                        .size(12.0)
                        .color(self.muted()),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.preview.is_some() {
                    if ui.small_button("Вписать").clicked() {
                        self.zoom = 1.0;
                    }
                    ui.add(
                        egui::Slider::new(&mut self.zoom, 0.5..=3.0)
                            .show_value(false)
                            .text("Масштаб"),
                    );
                }
            });
        });
        if !self.history.is_empty() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("НЕДАВНОЕ")
                        .size(10.0)
                        .strong()
                        .color(self.muted()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.ready.is_some() {
                        ui.label(
                            RichText::new("Следующая уже готова")
                                .size(11.0)
                                .color(self.ac_text()),
                        );
                    } else if self.net_busy && !self.waiting {
                        ui.label(
                            RichText::new("Предзагрузка…")
                                .size(11.0)
                                .color(self.muted()),
                        );
                    }
                });
            });
            let mut clicked = None;
            egui::ScrollArea::horizontal()
                .id_salt("history")
                .max_height(88.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (index, item) in self.history.iter().enumerate() {
                            let size = egui::vec2(86.0, 68.0);
                            let response = ui
                                .add(
                                    egui::ImageButton::new(
                                        egui::Image::new((item.thumb.id(), item.thumb.size_vec2()))
                                            .fit_to_exact_size(size)
                                            .maintain_aspect_ratio(true),
                                    )
                                    .selected(self.selected == Some(index))
                                    .frame(true),
                                )
                                .on_hover_text(format!(
                                    "{} × {} · {}",
                                    item.dimensions[0], item.dimensions[1], item.meta.tag
                                ));
                            if response.clicked() {
                                clicked = Some(index);
                            }
                        }
                    });
                });
            if let Some(index) = clicked {
                self.select(index);
            }
        }
    }
    fn collection(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.library_search)
                    .hint_text("Поиск по имени файла…")
                    .desired_width(260.0),
            );
            if ui.add(icons::button("Обновить", Icon::Refresh)).clicked() {
                self.thumbs.clear();
                self.thumb_failed.clear();
                self.scan();
            }
            ui.add(
                egui::Slider::new(&mut self.gallery_tile_width, 160.0..=320.0)
                    .text("Размер")
                    .show_value(false),
            );
            ui.label(format!("{} фото", self.library.len()));
            if self.scanning {
                ui.spinner();
            }
        });
        ui.add_space(12.0);
        let search = self.library_search.to_lowercase();
        let entries = self
            .library
            .iter()
            .filter(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&search)
            })
            .cloned()
            .map(GalleryEntry::Local)
            .collect();
        self.gallery_grid(ui, entries, "saved-grid");
    }
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let appearance_before = cloud::Appearance::from_settings(&self.settings);
        let before = serde_json::to_string(&self.settings).unwrap_or_default();
        let old_query = self.query();
        let old_roots = self.settings.library_roots(&self.base);
        let mut open = true;
        egui::Window::new("Настройки").open(&mut open).default_width(560.0).default_height(430.0)
            .default_pos(ctx.screen_rect().center()-egui::vec2(280.0,230.0)).resizable(false).collapsible(false).show(ctx,|ui|{
                ui.horizontal(|ui|{for (i,label) in [(0,"Оформление"),(1,"Сохранение"),(2,"Дополнительно"),(3,"Источники")]{ui.selectable_value(&mut self.settings_tab,i,label);}});
                ui.add_space(16.0);
                egui::ScrollArea::vertical().max_height(470.0).show(ui,|ui|{
                    ui.set_min_height(300.0);
                    match self.settings_tab{
                        0=>{
                            ui.label(RichText::new("Сделай галерею своей").size(24.0).strong());ui.label("Все изменения видны сразу.");ui.add_space(18.0);
                            ui.strong("Тема");ui.horizontal(|ui|{for (value,label) in [(Theme::Light,"Светлая"),(Theme::Dark,"Тёмная"),(Theme::System,"Как в Windows")]{ui.selectable_value(&mut self.settings.theme,value,label);}});
                            ui.add_space(16.0);ui.strong("Акцентный цвет");ui.checkbox(&mut self.settings.system_accent,"Использовать цвет Windows");
                            ui.horizontal(|ui|{for rgb in [[159,123,255],[235,110,171],[76,164,255],[55,187,157],[245,169,77],[233,103,104]]{if ui.add(egui::Button::new("   ").fill(Color32::from_rgb(rgb[0],rgb[1],rgb[2])).min_size(egui::vec2(38.0,34.0))).on_hover_text(format!("#{:02X}{:02X}{:02X}",rgb[0],rgb[1],rgb[2])).clicked(){self.settings.accent=rgb;self.settings.system_accent=false;}}if ui.color_edit_button_srgb(&mut self.settings.accent).changed(){self.settings.system_accent=false;}});
                            ui.add_space(16.0);if ui.button("Сбросить оформление").clicked(){self.settings.theme=Theme::System;self.settings.accent=[159,123,255];self.settings.system_accent=true;}
                            if self.cloud_user.is_some(){ui.small("Тема и акцент синхронизируются с аккаунтом.");}
                        },
                        1=>{
                            ui.label(RichText::new("Куда сохранять картинки").size(24.0).strong());ui.add_space(16.0);
                            ui.radio_value(&mut self.settings.save_mode,"A".into(),"Распределять по режиму и категории");ui.radio_value(&mut self.settings.save_mode,"B".into(),"В одну папку images");ui.radio_value(&mut self.settings.save_mode,"C".into(),"В выбранную папку");
                            if self.settings.save_mode=="C"{ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut self.settings.custom_save_folder).hint_text("Путь к папке").desired_width(350.0));if ui.button("Выбрать…").clicked(){if let Some(path)=rfd::FileDialog::new().pick_folder(){self.settings.custom_save_folder=path.to_string_lossy().into();}}});}
                            ui.add_space(10.0);ui.label(RichText::new("PNG в полном разрешении. Существующие файлы не заменяются.").size(12.0).color(self.muted()));
                            ui.add_space(16.0);ui.strong("Показывать в разделе «Сохранённое»");
                            egui::ComboBox::from_id_salt("load-mode").selected_text(match self.settings.load_saved_mode{1=>"Текущую категорию",2=>"Все категории текущего режима",_=>"Все изображения"}).show_ui(ui,|ui|{ui.selectable_value(&mut self.settings.load_saved_mode,1,"Текущую категорию");ui.selectable_value(&mut self.settings.load_saved_mode,2,"Все категории текущего режима");ui.selectable_value(&mut self.settings.load_saved_mode,3,"Все изображения");});
                            ui.small("Пути к папкам и скачанные файлы остаются на этом устройстве.");
                        },
                        2=>{
                            ui.label(RichText::new("Загрузка и управление").size(24.0).strong());ui.add_space(16.0);
                            if ui.checkbox(&mut self.settings.prefetch,"Предзагружать соседние фотографии").changed(){self.queue_favorite_thumbnails();}ui.small("Ускоряет листание просмотра, избранного и альбомов. Расходует трафик.");
                            
                            ui.add_space(18.0);self.update_settings_ui(ui,ctx);
                            ui.add_space(18.0);ui.strong("Горячие клавиши");ui.label("Пробел / N — новая картинка\nS — сохранить\n← / → — история\nCtrl+, — настройки");
                        }
                        _=>{self.sources_ui(ui);}
                    }
                });
            });
        self.settings_open = open;
        let appearance_after = cloud::Appearance::from_settings(&self.settings);
        if appearance_before != appearance_after && self.cloud_user.is_some() {
            self.pending_appearance = Some(appearance_after);
            self.appearance_retry = Instant::now() + Duration::from_millis(600);
            self.cloud_status = "Оформление ожидает синхронизации".into();
        }
        if serde_json::to_string(&self.settings).unwrap_or_default() != before {
            self.changed();
            if self.query() != old_query {
                self.invalidate();
            } else if self.settings.library_roots(&self.base) != old_roots {
                self.scan();
            }
            if !self.settings.prefetch {
                self.ready = None;
            }
        }
    }
    fn housekeeping(&mut self, ctx: &egui::Context) {
        self.update_events();
        if !self.smoke
            && self.settings.auto_update
            && self.started.elapsed() > Duration::from_secs(3)
            && !self.update_busy
            && self.update_ready.is_none()
            && crate::updater::now().saturating_sub(self.settings.update_last_check) > 43200
        {
            self.check_updates();
        }
        if self.cloud_user.is_some() && !self.cloud_busy {
            if self.pending_appearance.is_some() && Instant::now() >= self.appearance_retry {
                let a = self.pending_appearance.clone().unwrap();
                self.cloud_send(CloudJob::Op(
                    serde_json::json!({"op":"appearance","appearance":a}),
                ));
                self.appearance_retry = Instant::now() + Duration::from_secs(30);
            } else if self.cloud_last.elapsed() > Duration::from_secs(30) {
                self.cloud_send(CloudJob::Refresh);
            }
        }
        if self.scan_due
            && !self.scanning
            && self
                .workers
                .disk
                .try_send(DiskJob::Scan(
                    self.library_epoch,
                    self.settings.library_roots(&self.base),
                    {let mut settings=self.settings.clone();settings.load_saved_mode=3;settings.library_roots(&self.base)},
                ))
                .is_ok()
        {
            self.scanning = true;
            self.scan_due = false;
        }
        if self.last_os_check.elapsed() > Duration::from_secs(2)
            && !self.os_pending
            && self.workers.disk.try_send(DiskJob::Appearance).is_ok()
        {
            self.os_pending = true;
            self.last_os_check = Instant::now();
        }
        if self
            .dirty
            .is_some_and(|t| t.elapsed() > Duration::from_millis(400))
            && self
                .workers
                .disk
                .try_send(DiskJob::Settings(self.settings.clone(), self.base.clone()))
                .is_ok()
        {
            self.dirty = None;
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        if self.dirty.is_some() {
            ctx.request_repaint_after(Duration::from_millis(400));
        }
    }
    fn smoke(&mut self, ctx: &egui::Context) {
        if !self.smoke {
            return;
        }
        if self.started.elapsed() > Duration::from_secs(90) {
            let _ = std::fs::write(
                self.base.join("smoke-result.txt"),
                format!("FAILED: {}", self.status),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(50));
        let screenshot = ctx.input(|i| {
            i.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = screenshot {
            let name = match self.smoke_phase {
                4 => "preview-dark.png",
                6 => "preview-light.png",
                8 => "preview-settings.png",
                10 => "preview-collection.png",
                14 => "preview-account.png",
                17 => "preview-gallery.png",
                20 => "preview-sources.png",
                22 => "preview-favorites.png",
                _ => "preview.png",
            };
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            let _ = image::save_buffer(
                self.base.join(name),
                &rgba,
                image.width() as u32,
                image.height() as u32,
                image::ColorType::Rgba8,
            );
            match self.smoke_phase {
                4 => {
                    self.settings.theme = Theme::Light;
                    self.smoke_phase = 5;
                }
                6 => {
                    self.settings_open = true;
                    self.capture_after = Instant::now() + Duration::from_millis(400);
                    self.smoke_phase = 7;
                }
                8 => {
                    self.settings_tab=3;self.capture_after=Instant::now()+Duration::from_millis(400);self.smoke_phase=19;
                }
                20 => {
                    self.settings_open = false;
                    self.account_open = true;
                    self.capture_after = Instant::now() + Duration::from_millis(400);
                    self.smoke_phase = 13;
                }
                22 => {self.smoke_phase=12;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}
                17 => {
                    self.smoke_phase = 18;
                }
                14 => {
                    self.account_open = false;
                    self.view = View::Collection;
                    self.smoke_phase = 9;
                }
                10 => {
                    let path=self.smoke_path.as_ref().unwrap().clone();
                    if !self.settings.local_favorites.contains(&path){self.toggle_local_favorite(path);}
                    self.open_local(self.smoke_path.as_ref().unwrap().clone());
                    self.smoke_phase = 11;
                }
                _ => {}
            }
            ctx.request_repaint();
            return;
        }
        match self.smoke_phase {
            0 if !self.tags_loading => {
                self.next(ctx);
                self.smoke_phase = 1;
            }
            1 if self.selected.is_some() && !self.waiting => {
                if let Some(path)=self.saved_sources.get(&self.history[self.selected.unwrap()].meta.key).and_then(|paths|paths.first()).cloned(){self.smoke_path=Some(path);}else{self.save_current();}
                self.smoke_phase = 2;
            }
            2 if self.smoke_path.is_some() && !self.scanning && !self.scan_due => {
                self.settings.theme = Theme::Dark;
                self.settings.system_accent = false;
                self.smoke_phase = 3;
                ctx.request_repaint();
            }
            3 if self.style_key.is_some_and(|s| s.0) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_phase = 4;
            }
            5 if self.style_key.is_some_and(|s| !s.0) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_phase = 6;
            }
            7 if Instant::now() >= self.capture_after => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_phase = 8;
            }
            21 if Instant::now() >= self.capture_after && self.cloud_data.favorites.first().is_some_and(|m|self.remote_thumbs.contains_key(&m.key)) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));self.smoke_phase=22;
            }
            19 if Instant::now() >= self.capture_after => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));self.smoke_phase=20;
            }
            13 if Instant::now() >= self.capture_after => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_phase = 14;
            }
            9 if self
                .library
                .iter()
                .take(12)
                .all(|p| self.thumbs.contains_key(p)) =>
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_phase = 10;
            }
            11 if self
                .lightbox
                .as_ref()
                .is_some_and(|g| g.loaded().is_some() && !g.busy && (!self.settings.prefetch || g.neighbors().iter().all(|&i|g.cache.iter().any(|(k,_)|k==&g.entries[i].key())))) =>
            {
                self.gallery_step(1);
                self.smoke_phase = 15;
            }
            15 if self
                .lightbox
                .as_ref()
                .is_some_and(|g| g.loaded().is_some() && !g.busy) =>
            {
                self.gallery_step(-1);
                self.smoke_phase = 16;
            }
            16 if self
                .lightbox
                .as_ref()
                .is_some_and(|g| g.loaded().is_some() && !g.busy) =>
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_phase = 17;
            }
            18 if self
                .lightbox
                .as_ref()
                .is_some_and(|g| g.loaded().is_some() && !g.busy) =>
            {
                let loaded = self.lightbox.as_ref().unwrap().loaded().unwrap();
                let path = self.smoke_path.as_ref().unwrap();
                let ok = self.saved_sources.contains_key(&loaded.meta.key) && self.photo_favorite(&loaded.meta.key) && self.view == View::Collection
                    && self.history.len() == 1
                    && matches!(&loaded.source,Source::Disk(p) if p==path)
                    && image::image_dimensions(path)
                        .is_ok_and(|d| d == (loaded.dimensions[0], loaded.dimensions[1]))
                    && loaded.meta.key.starts_with("http");
                let _=std::fs::write(self.base.join("smoke-result.txt"),format!("{}: prefetch=true, saved_index=true, favorite=true, gallery_separate=true, next_previous=true, history_len={}, dimensions={:?}",if ok{"OK"}else{"FAILED"},self.history.len(),loaded.dimensions));
                let mut remote=loaded.meta.clone();remote.key.push_str("#thumb-smoke");self.cloud_data.favorites=vec![remote];self.queue_favorite_thumbnails();
                self.lightbox=None;self.workers.prefetch_generation.store(0,std::sync::atomic::Ordering::Relaxed);self.view=View::Favorites;self.capture_after=Instant::now()+Duration::from_millis(400);self.smoke_phase=21;
            }
            _ => {}
        }
    }
}

impl eframe::App for GalleryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.events(ctx);
        self.cloud_events();
        self.warm_favorite_thumbnails();
        if ctx.input(|i| i.viewport().close_requested()) && !self.saving.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
            self.message("Завершаем сохранение перед закрытием…", false);
        }
        if self.closing && self.saving.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.housekeeping(ctx);
        self.apply_style(ctx);
        if self.lightbox.is_none() && !ctx.wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::N)) {
                self.next(ctx);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::S)) {
                self.save_current();
            }
            if let Some(index) = self.selected {
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                    self.select((index + 1).min(self.history.len() - 1));
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                    self.select(index.saturating_sub(1));
                }
            }
        }
        if self.lightbox.is_none()
            && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma))
        {
            self.settings_open = !self.settings_open;
        }
        self.sidebar(ctx);
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(self.background())
                    .inner_margin(egui::Margin::symmetric(22, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let color = if self.error {
                        if self.dark() {
                            Color32::from_rgb(235, 110, 120)
                        } else {
                            Color32::from_rgb(174, 40, 65)
                        }
                    } else {
                        self.muted()
                    };
                    ui.add(
                        egui::Label::new(RichText::new(&self.status).size(11.0).color(color))
                            .truncate(),
                    )
                    .on_hover_text(&self.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("ПРОБЕЛ  новая картинка     S  сохранить")
                                .size(10.0)
                                .color(self.muted()),
                        );
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.background()).inner_margin(22))
            .show(ctx, |ui| {
                self.header(ui, ctx);
                self.update_banner(ui, ctx);
                match self.view {
                    View::Discover => self.viewer(ui),
                    View::Collection => self.collection(ui),
                    View::Favorites | View::Albums => self.cloud_library(ui),
                }
            });
        self.settings_window(ctx);
        self.filters_window(ctx);
        self.account_window(ctx);
        self.gallery_modal(ctx);
        self.smoke(ctx);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Keep settings writes serialized with the disk worker.
        let (tx, rx) = std::sync::mpsc::channel();
        if self
            .workers
            .disk
            .send(DiskJob::Settings(self.settings.clone(), self.base.clone()))
            .is_ok()
            && self.workers.disk.send(DiskJob::Flush(tx)).is_ok()
        {
            let _ = rx.recv_timeout(Duration::from_secs(30));
        }
        if self.install_after_exit {
            if let Ok(exe) = std::env::current_exe() {
                let mut cmd = std::process::Command::new(exe);
                cmd.arg("--install");
                crate::install::hidden(&mut cmd);
                let _ = cmd.spawn();
            }
        } else if self.settings.auto_update || self.update_restart {
            if let Some(ready) = &self.update_ready {
                if let Err(e) = crate::updater::schedule(ready, &self.base, self.update_restart) {
                    let _ = std::fs::write(self.base.join("update-error.txt"), e);
                }
            }
        }
    }
}

include!("cloud_ui.rs");
include!("update_ui.rs");
include!("gallery_ui.rs");

fn palette(settings: &Settings, appearance: Appearance) -> (bool, [u8; 3]) {
    let dark = match settings.theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => !appearance.light,
    };
    let accent = if settings.system_accent {
        appearance.accent.unwrap_or(settings.accent)
    } else {
        settings.accent
    };
    (dark, accent)
}
fn trim_history(history: &mut Vec<Item>) {
    while history.len() > HISTORY_LIMIT
        || (history.len() > 1
            && history
                .iter()
                .map(|i| i.source.memory_bytes())
                .sum::<usize>()
                > MEMORY_LIMIT)
    {
        history.pop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_theme_and_accent_follow_os_changes() {
        let mut settings = Settings::default();
        let dark = Appearance {
            light: false,
            accent: Some([10, 20, 30]),
        };
        let light = Appearance {
            light: true,
            accent: Some([200, 100, 80]),
        };
        assert_eq!(palette(&settings, dark), (true, [10, 20, 30]));
        assert_eq!(palette(&settings, light), (false, [200, 100, 80]));
        settings.theme = Theme::Dark;
        settings.system_accent = false;
        settings.accent = [120, 80, 240];
        assert_eq!(palette(&settings, light), (true, [120, 80, 240]));
        assert_eq!(palette(&settings, dark), (true, [120, 80, 240]));
        settings.system_accent = true;
        assert_eq!(
            palette(
                &settings,
                Appearance {
                    light: false,
                    accent: None
                }
            )
            .1,
            settings.accent
        );
    }
    #[test]
    fn history_releases_oldest_encoded_images() {
        let ctx = egui::Context::default();
        let thumb = ctx.load_texture(
            "test",
            egui::ColorImage::new([1, 1], Color32::BLACK),
            TextureOptions::LINEAR,
        );
        let source = Source::Memory(std::sync::Arc::new(vec![0; 4 * 1024 * 1024]));
        let mut history: Vec<Item> = (0..100)
            .map(|i| Item {
                meta: Metadata {
                    key: i.to_string(),
                    source: None,
                    artist: String::new(),
                    mode: "SFW".into(),
                    tag: "waifu".into(),
                },
                source: source.clone(),
                dimensions: [1, 1],
                thumb: thumb.clone(),
                saved: false,
            })
            .collect();
        trim_history(&mut history);
        assert_eq!(history.len(), 24);
        assert_eq!(history[0].meta.key, "0");
        assert_eq!(history.last().unwrap().meta.key, "23");
    }
}

include!("source_ui.rs");

include!("photo_state.rs");
