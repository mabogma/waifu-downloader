impl GalleryApp {
    fn open_gallery(&mut self, entries: Vec<GalleryEntry>, index: usize) {
        if entries.is_empty() || index >= entries.len() {
            return;
        }
        self.gallery_epoch+=1;
        self.workers.prefetch_generation.store(self.gallery_epoch,std::sync::atomic::Ordering::Relaxed);
        let mut viewer=GalleryViewer::new(entries,index);viewer.session=self.gallery_epoch;
        self.lightbox=Some(viewer);
        self.gallery_load_current();
    }
    fn gallery_load_current(&mut self) {
        let Some(g) = self.lightbox.as_mut() else {
            return;
        };
        if let Some(loaded) = g.loaded() {
            if g.texture.is_none() {
                let preview = loaded.preview.clone();
                g.texture = Some(self.gallery_context.load_texture(
                    "gallery-preview",
                    preview,
                    TextureOptions::LINEAR,
                ));
            }
            return;
        }
        if g.busy || g.error.is_some() || g.pending.contains(&g.entries[g.index].key()) {
            return;
        }
        let entry = g.entries[g.index].clone();
        self.gallery_epoch += 1;
        let epoch = self.gallery_epoch;
        let sent = match entry {
            GalleryEntry::Local(path) => self
                .workers
                .disk
                .try_send(DiskJob::Gallery(epoch, path))
                .is_ok(),
            GalleryEntry::Remote(meta) => self
                .workers
                .net
                .try_send(NetJob::Gallery(epoch, meta))
                .is_ok(),
        };
        if sent {
            g.epoch = epoch;
            g.requested_index = g.index;
            g.busy = true;
        }
    }
    fn gallery_result(
        &mut self,
        ctx: &egui::Context,
        epoch: u64,
        result: Result<Box<Loaded>, String>,
    ) {
        let Some(g) = self.lightbox.as_mut() else {
            return;
        };
        if g.epoch != epoch {
            return;
        }
        g.busy = false;
        match result {
            Ok(loaded) => {
                let key = g.entries[g.requested_index].key();
                g.insert(key, *loaded);
            }
            Err(error) => {
                if g.index == g.requested_index {
                    g.error = Some(error);
                }
            }
        }
        let _ = ctx;
        self.gallery_load_current();
    }
    fn gallery_step(&mut self, delta: isize) {
        if let Some(g) = self.lightbox.as_mut() {
            g.step(delta);
        }
        self.gallery_load_current();
    }
    fn gallery_prefetch_neighbors(&mut self) {
        if !self.settings.prefetch{return;}
        let Some(g)=self.lightbox.as_mut() else{return;};
        if g.busy||g.loaded().is_none()||g.pending.len()>=2{return;}
        for index in g.neighbors(){
            let entry=g.entries[index].clone();let key=entry.key();
            if g.cache.iter().any(|(k,_)|k==&key)||g.pending.contains(&key)||g.prefetch_seen.contains(&key){continue;}
            if self.workers.gallery_prefetch.try_send((g.session,entry)).is_ok(){g.pending.insert(key.clone());g.prefetch_seen.insert(key);}else{break;}
        }
    }
    fn gallery_prefetch_result(&mut self,session:u64,key:String,result:Result<Box<Loaded>,String>){
        let Some(g)=self.lightbox.as_mut() else{return;};
        if g.session!=session{return;}
        g.pending.remove(&key);
        if let Ok(loaded)=result{
            if g.entries[g.index].key()==key||g.neighbors().iter().any(|&i|g.entries[i].key()==key){g.insert(key,*loaded);}
        }
        self.gallery_load_current();
    }
    fn gallery_grid(&mut self, ui: &mut egui::Ui, entries: Vec<GalleryEntry>, id: &str) {
        if entries.is_empty() {
            ui.add_space(70.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Здесь пока нет картинок").size(24.0).strong());
                ui.label("Сохраняй находки и добавляй их в избранное.");
            });
            return;
        }
        let columns = (ui.available_width() / self.gallery_tile_width)
            .floor()
            .max(1.0) as usize;
        let width = (ui.available_width() - (columns - 1) as f32 * 12.0) / columns as f32;
        let image_height = (width * 1.03).clamp(150.0, 300.0);
        let row_height = image_height + 126.0;
        let mut clicked = None;
        egui::ScrollArea::vertical().id_salt(id).show_rows(
            ui,
            row_height,
            entries.len().div_ceil(columns),
            |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        for index in row * columns..((row + 1) * columns).min(entries.len()) {
                            let entry = &entries[index];
                            ui.push_id(entry.key(), |ui| {
                                egui::Frame::new()
                                    .fill(self.panel())
                                    .corner_radius(16)
                                    .inner_margin(10)
                                    .show(ui, |ui| {
                                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                        ui.set_width((width - 20.0).max(100.0));
                                        let texture = match entry {
                                            GalleryEntry::Local(path) => {
                                                if !self.remote_thumbs.contains_key(&path.to_string_lossy().to_string()) && !self.thumbs.contains_key(path)
                                                    && !self.thumb_failed.contains(path)
                                                    && !self.thumb_pending.contains(path)
                                                    && self
                                                        .workers
                                                        .disk
                                                        .try_send(DiskJob::Thumb(path.clone()))
                                                        .is_ok()
                                                {
                                                    self.thumb_pending.insert(path.clone());
                                                }
                                                self.thumbs.get(path).cloned().or_else(||self.remote_thumbs.get(&path.to_string_lossy().to_string()).cloned())
                                            }
                                            GalleryEntry::Remote(meta) => {
                                                let texture = self
                                                    .history
                                                    .iter()
                                                    .find(|h| h.meta.key == meta.key)
                                                    .map(|h| h.thumb.clone())
                                                    .or_else(|| {
                                                        self.remote_thumbs.get(&meta.key).cloned()
                                                    });
                                                if texture.is_none()
                                                    && !self.remote_pending.contains(&meta.key)
                                                    && !self.remote_failed.contains(&meta.key)
                                                    && self
                                                        .workers
                                                        .remote_thumbs
                                                        .try_send((meta.clone(),self.saved_sources.get(&meta.key).and_then(|p|p.first()).cloned()))
                                                        .is_ok()
                                                {
                                                    self.remote_pending.insert(meta.key.clone());
                                                }
                                                texture
                                            }
                                        };
                                        let (rect, response) = ui.allocate_exact_size(
                                            egui::vec2((width - 20.0).max(100.0), image_height),
                                            egui::Sense::click(),
                                        );
                                        ui.painter().rect_filled(rect, 10, self.card());
                                        if let Some(texture) = texture {
                                            let ratio = rect.width() / rect.height();
                                            let source = texture.size_vec2();
                                            let aspect = source.x / source.y;
                                            let uv = if aspect > ratio {
                                                let w = ratio / aspect;
                                                egui::Rect::from_min_max(
                                                    egui::pos2((1.0 - w) / 2.0, 0.0),
                                                    egui::pos2((1.0 + w) / 2.0, 1.0),
                                                )
                                            } else {
                                                let h = aspect / ratio;
                                                egui::Rect::from_min_max(
                                                    egui::pos2(0.0, (1.0 - h) / 2.0),
                                                    egui::pos2(1.0, (1.0 + h) / 2.0),
                                                )
                                            };
                                            ui.painter().image(
                                                texture.id(),
                                                rect,
                                                uv,
                                                Color32::WHITE,
                                            );
                                        } else {
                                            icons::paint(
                                                ui,
                                                Icon::Image,
                                                rect.center(),
                                                30.0,
                                                self.muted(),
                                            );
                                        }
                                        if response.hovered() {
                                            ui.painter().rect_stroke(
                                                rect,
                                                10,
                                                egui::Stroke::new(2.0, self.ac()),
                                                egui::StrokeKind::Inside,
                                            );
                                        }
                                        if response
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            clicked = Some(index);
                                        }
                                        self.photo_badges(ui, entry);
                                        let (title, subtitle) = match entry {
                                            GalleryEntry::Local(p) => (
                                                p.file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy()
                                                    .into_owned(),
                                                "На компьютере".to_string(),
                                            ),
                                            GalleryEntry::Remote(m) => (
                                                if m.artist.is_empty() {
                                                    tag_label(&m.tag)
                                                } else {
                                                    m.artist.clone()
                                                },
                                                format!(
                                                    "{} · {}",
                                                    tag_label(&m.tag),
                                                    if m.mode == "NSFW" {
                                                        "18+"
                                                    } else {
                                                        "Избранное"
                                                    }
                                                ),
                                            ),
                                        };
                                        ui.add(
                                            egui::Label::new(RichText::new(title).strong())
                                                .truncate(),
                                        );
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(subtitle)
                                                    .size(11.0)
                                                    .color(self.muted()),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui.small_button("Открыть").clicked() {
                                                        clicked = Some(index);
                                                    }
                                                    ui.menu_button("…", |ui| {
                                                        self.gallery_entry_actions(ui, entry);
                                                    });
                                                },
                                            );
                                        });
                                        });
                                    });
                            });
                        }
                    });
                }
            },
        );
        if let Some(index) = clicked {
            self.open_gallery(entries, index);
        }
    }
    fn gallery_entry_actions(&mut self, ui: &mut egui::Ui, entry: &GalleryEntry) {
        match entry {
            GalleryEntry::Local(path) => {
                let favorite = self.settings.local_favorites.contains(path);
                if ui
                    .button(if favorite {
                        "Убрать из локального избранного"
                    } else {
                        "В локальное избранное"
                    })
                    .clicked()
                {
                    self.toggle_local_favorite(path.clone());
                    ui.close_menu();
                }
            }
            GalleryEntry::Remote(meta) => {
                let albums = self.cloud_data.albums.clone();
                ui.add_enabled_ui(!self.cloud_busy,|ui|{
                    ui.menu_button("Добавить в альбом",|ui|{if albums.is_empty(){ui.label("Создай альбом в разделе «Альбомы»");}for a in albums{if ui.button(&a.name).clicked(){self.cloud_send(CloudJob::Op(serde_json::json!({"op":"album_add","id":a.id,"key":meta.key})));ui.close_menu();}}});
                    if self.view==View::Albums{if ui.button("Убрать из альбома").clicked(){self.cloud_send(CloudJob::Op(serde_json::json!({"op":"album_remove","id":self.album_selected,"key":meta.key})));ui.close_menu();}}
                    if ui.button("Убрать из избранного").clicked(){self.cloud_send(CloudJob::Op(serde_json::json!({"op":"favorite_remove","key":meta.key})));ui.close_menu();}
                });
            }
        }
    }
    fn gallery_save(&mut self) {
        let Some(loaded) = self.lightbox.as_ref().and_then(|g| g.loaded()) else {
            return;
        };
        if matches!(loaded.source, Source::Disk(_)) || self.saved_sources.contains_key(&loaded.meta.key) || self.saving.contains(&loaded.meta.key) {
            return;
        }
        let meta = loaded.meta.clone();
        let source = loaded.source.clone();
        let folder = self.settings.save_folder(&self.base, &meta.mode, &meta.tag);
        if self
            .workers
            .disk
            .try_send(DiskJob::SaveWithMetadata(meta.clone(), source, folder))
            .is_ok()
        {
            self.saving.insert(meta.key);
        }
    }
    fn gallery_modal(&mut self, ctx: &egui::Context) {
        if self.lightbox.is_none() {
            return;
        }
        self.gallery_load_current();
        self.gallery_prefetch_neighbors();
        if !ctx.wants_keyboard_input() && !ctx.memory(|m| m.any_popup_open()) {
            if ctx
                .input(|i| i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::Space))
            {
                self.gallery_step(1);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                self.gallery_step(-1);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::S)) {
                self.gallery_save();
            }
        }
        let mut close = false;
        let mut step = 0;
        let mut save = false;
        let mut favorite = false;
        let screen = ctx.screen_rect();
        let width = (screen.width() - 60.0).max(500.0);
        let height = (screen.height() - 80.0).max(450.0);
        let response=egui::Modal::new(egui::Id::new("gallery-lightbox")).backdrop_color(Color32::from_black_alpha(200)).frame(egui::Frame::new().fill(self.panel()).corner_radius(18).inner_margin(18)).show(ctx,|ui|{
            ui.set_width(width-36.0);ui.set_min_height(height-36.0);
            let g=self.lightbox.as_ref().unwrap();
            ui.horizontal(|ui|{
                if ui.add(icons::button("К миниатюрам",Icon::Collection)).clicked(){close=true;}
                ui.label(RichText::new(format!("{} / {}",g.index+1,g.entries.len())).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if ui.button("Следующая").clicked(){step=1;}if ui.button("Предыдущая").clicked(){step=-1;}});
            });ui.add_space(8.0);
            let canvas=egui::vec2(ui.available_width(),(height-260.0).max(180.0));
            let error=self.lightbox.as_ref().unwrap().error.clone();
            let texture=self.lightbox.as_ref().unwrap().texture.clone();
            let zoom=self.lightbox.as_ref().unwrap().zoom;
            if let Some(error)=error{ui.allocate_ui_with_layout(canvas,egui::Layout::top_down(egui::Align::Center),|ui|{ui.add_space(canvas.y*0.35);ui.label(error);if ui.button("Повторить").clicked(){self.lightbox.as_mut().unwrap().error=None;}});}
            else if let Some(texture)=texture{let source=texture.size_vec2();let size=source*(canvas.x/source.x).min(canvas.y/source.y)*zoom;
                egui::ScrollArea::both().id_salt("lightbox-image").max_height(canvas.y).auto_shrink([false,false]).show(ui,|ui|{
                    ui.allocate_ui_with_layout(egui::vec2(canvas.x.max(size.x),canvas.y.max(size.y)),egui::Layout::top_down(egui::Align::Center),|ui|{ui.add_space(((canvas.y-size.y)/2.0).max(0.0));let image=ui.add(egui::Image::new((texture.id(),size)).corner_radius(10).sense(egui::Sense::click()));if image.double_clicked(){let g=self.lightbox.as_mut().unwrap();g.zoom=if g.zoom>1.0{1.0}else{2.0};}});
                });
            }else{ui.allocate_ui_with_layout(canvas,egui::Layout::top_down(egui::Align::Center),|ui|{ui.add_space(canvas.y*0.4);ui.spinner();ui.label("Загружаем картинку…");});ctx.request_repaint_after(Duration::from_millis(100));}
            ui.add_space(8.0);
            let details=self.lightbox.as_ref().and_then(|g|g.loaded()).map(|l|(l.meta.clone(),l.source.clone(),l.dimensions));
            if let Some((meta,source,dimensions))=details{
                let local=!(meta.key.starts_with("http://")||meta.key.starts_with("https://"));let starred=if local{matches!(&source,Source::Disk(p) if self.settings.local_favorites.contains(p))}else{self.photo_favorite(&meta.key)};
                ui.horizontal_wrapped(|ui|{
                    ui.label(format!("{} × {}",dimensions[0],dimensions[1]));if !meta.artist.is_empty(){ui.label(&meta.artist);}if let Some(url)=&meta.source{ui.hyperlink_to("Источник",url);}

                    if ui.add_enabled(local||!self.cloud_busy,icons::button(if starred{"В избранном"}else{"В избранное"},if starred{Icon::HeartFilled}else{Icon::Heart})).clicked(){favorite=true;}
                    if ui.add_enabled(!matches!(source,Source::Disk(_))&&!self.saved_sources.contains_key(&meta.key)&&!self.saving.contains(&meta.key),icons::button(if matches!(source,Source::Disk(_))||self.saved_sources.contains_key(&meta.key){"Сохранено"}else{"Сохранить"},Icon::Download)).clicked(){save=true;}
                    ui.add(egui::Slider::new(&mut self.lightbox.as_mut().unwrap().zoom,1.0..=4.0).text("Масштаб").show_value(false));
                });
                if favorite{
                    self.toggle_photo_favorite(meta,match source{Source::Disk(path)=>Some(path),_=>None});
                    if self.account_open {close=true;}
                }
            }
            ui.add_space(8.0);
            let g=self.lightbox.as_ref().unwrap();let start=g.index.saturating_sub(4);let end=(start+9).min(g.entries.len());
            ui.horizontal(|ui|{for index in start..end{
                let g=self.lightbox.as_ref().unwrap();let selected=index==g.index;let entry=g.entries[index].clone();
                let texture=match &entry{GalleryEntry::Local(p)=>self.thumbs.get(p).cloned(),GalleryEntry::Remote(m)=>self.remote_thumbs.get(&m.key).cloned().or_else(||self.history.iter().find(|h|h.meta.key==m.key).map(|h|h.thumb.clone()))};
                let (rect,response)=ui.allocate_exact_size(egui::vec2(45.0,52.0),egui::Sense::click());ui.painter().rect_filled(rect,6,self.card());
                if let Some(t)=texture{let fit=(rect.width()/t.size_vec2().x).min(rect.height()/t.size_vec2().y);let r=egui::Rect::from_center_size(rect.center(),t.size_vec2()*fit);ui.painter().image(t.id(),r,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),Color32::WHITE);}else{icons::paint(ui,Icon::Image,rect.center(),18.0,self.muted());}
                if selected{ui.painter().rect_stroke(rect,6,egui::Stroke::new(2.0,self.ac()),egui::StrokeKind::Inside);}
                if response.on_hover_text(format!("Фото {}",index+1)).clicked(){let g=self.lightbox.as_mut().unwrap();g.index=index;g.prefetch_seen.clear();g.texture=None;g.error=None;g.zoom=1.0;}
            }});
            ui.label(RichText::new("Стрелки — листать   ·   Esc — к миниатюрам   ·   двойной щелчок — масштаб").size(11.0).color(self.muted()));
        });
        if save {
            self.gallery_save();
        }
        if step != 0 {
            self.gallery_step(step);
        }
        if close || response.should_close() {
            self.workers.prefetch_generation.store(0,std::sync::atomic::Ordering::Relaxed);
            self.lightbox = None;
            self.gallery_epoch += 1;
        }
    }
}
