impl GalleryApp {
    fn cloud_send(&mut self, job: CloudJob) {
        if self.cloud_busy {
            return;
        }
        if self.cloud.tx.try_send(job).is_ok() {
            self.cloud_busy = true;
            self.cloud_last = Instant::now();
            self.cloud_status = "Синхронизация…".into();
            self.cloud_error = false;
        } else {
            self.cloud_status = "Очередь занята. Повтори действие".into();
            self.cloud_error = true;
        }
    }
    fn receive_cloud_state(&mut self, data: cloud::Data) {
        if let Some(wanted) = &self.pending_appearance {
            if data.appearance.as_ref() == Some(wanted) {
                self.pending_appearance = None;
            }
        } else if let Some(a) = &data.appearance {
            a.apply(&mut self.settings);
            self.changed();
        }
        if self
            .album_selected
            .as_ref()
            .is_some_and(|id| !data.albums.iter().any(|a| &a.id == id))
        {
            self.album_selected = None;
            self.album_rename.clear();
        }
        self.cloud_data = data;
        self.remote_failed.clear();
        self.queue_favorite_thumbnails();
    }
    fn cloud_events(&mut self) {
        while let Ok(event) = self.cloud.rx.try_recv() {
            self.cloud_busy = false;
            self.cloud_last = Instant::now();
            match event {
                cloud::Event::Connected {
                    username,
                    state,
                    remembered,
                } => {
                    self.login_name = username.clone();
                    self.cloud_user = Some(username);
                    self.pending_appearance = None;
                    if state.appearance.is_none() {
                        self.pending_appearance =
                            Some(cloud::Appearance::from_settings(&self.settings));
                        self.appearance_retry = Instant::now();
                    }
                    self.receive_cloud_state(state);
                    self.cloud_error = !remembered;
                    self.cloud_status=if remembered{"Вход выполнен. Избранное, альбомы и оформление синхронизируются"}else{"Вход выполнен. Windows не сохранила сессию; при следующем запуске войди снова"}.into();
                }
                cloud::Event::State(data) => {
                    self.receive_cloud_state(data);
                    self.cloud_error = false;
                    self.cloud_status = "Синхронизировано".into();
                }
                cloud::Event::LoggedOut => {
                    self.remote_thumbs.clear();
                    self.remote_failed.clear();
                    self.cloud_user = None;
                    self.cloud_data = cloud::Data::default();
                    self.pending_appearance = None;
                    self.album_selected = None;
                    self.cloud_error = false;
                    self.cloud_status =
                        "Войди, чтобы открыть коллекцию на других устройствах".into();
                }
                cloud::Event::Error(message, expired) => {
                    if expired {
                        self.cloud_user = None;
                        self.cloud_data = cloud::Data::default();
                        self.pending_appearance = None;
                    }
                    self.cloud_status = message;
                    self.cloud_error = true;
                }
            }
        }
    }
    fn account_window(&mut self, ctx: &egui::Context) {
        if !self.account_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Аккаунт")
            .default_pos(ctx.screen_rect().center()-egui::vec2(245.0,230.0)).open(&mut open).collapsible(false).resizable(false).default_width(470.0).show(ctx,|ui|{
            ui.label(RichText::new(if self.cloud_user.is_some(){"Твоя коллекция — с тобой"}else{"Сохрани свои находки"}).size(24.0).strong());
            ui.label("Избранное, альбомы и оформление на всех устройствах.");ui.add_space(14.0);
            if let Some(username)=self.cloud_user.clone(){
                ui.strong(format!("Выполнен вход: {username}"));
                ui.label(format!("{} изображений · {} альбомов",self.cloud_data.favorites.len(),self.cloud_data.albums.len()));
                ui.label("Скачанные файлы остаются на этом компьютере.");
                ui.add_space(10.0);ui.horizontal(|ui|{
                    if ui.add_enabled(!self.cloud_busy,egui::Button::new("Синхронизировать")).clicked(){self.cloud_send(CloudJob::Refresh);}
                    if ui.add_enabled(!self.cloud_busy,egui::Button::new("Выйти")).clicked(){self.cloud_send(CloudJob::Logout);}
                });
            }else{
                ui.horizontal(|ui|{ui.selectable_value(&mut self.login_register,false,"Войти");ui.selectable_value(&mut self.login_register,true,"Создать аккаунт");});
                ui.label("Логин");ui.add_enabled(!self.cloud_busy,egui::TextEdit::singleline(&mut self.login_name).hint_text("От 3 символов: a-z, 0-9, _ или -").desired_width(f32::INFINITY));
                ui.label("Пароль");let password=ui.add_enabled(!self.cloud_busy,egui::TextEdit::singleline(&mut self.login_password).password(true).hint_text("Не менее 10 символов").desired_width(f32::INFINITY));
                if self.login_register{ui.label(RichText::new("Почта не нужна. Для восстановления пароля потребуется помощь владельца сервера.").size(12.0).color(self.muted()));}
                let enter=password.lost_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter));
                if self.primary(ui,if self.login_register{"Создать аккаунт"}else{"Войти"},!self.cloud_busy&&!self.login_name.trim().is_empty()&&!self.login_password.is_empty()).clicked()||enter&&!self.cloud_busy {
                    let password=std::mem::take(&mut self.login_password);
                    self.cloud_send(CloudJob::Auth{server:self.settings.sync_server.clone(),username:self.login_name.trim().into(),password,register:self.login_register});
                }
                ui.add_space(10.0);ui.collapsing("Адрес сервера",|ui|{
                    if ui.add_enabled(!self.cloud_busy,egui::TextEdit::singleline(&mut self.settings.sync_server).desired_width(f32::INFINITY)).changed(){self.changed();}
                    ui.small("Для твоего сервера: https://waifudownload.hopto.org");
                });
            }
            ui.add_space(14.0);
            if self.cloud_busy{ui.spinner();}
            ui.label(RichText::new(&self.cloud_status).color(if self.cloud_error{if self.dark(){Color32::from_rgb(255,140,150)}else{Color32::from_rgb(160,35,55)}}else{self.muted()}));
            if self.pending_appearance.is_some(){ui.small("Оформление ещё не отправлено. Для синхронизации оставь приложение открытым.");}
        });
        self.account_open = open;
    }
    fn cloud_library(&mut self, ui: &mut egui::Ui) {
        if self.cloud_user.is_none()
            && !(self.view == View::Favorites && !self.settings.local_favorites.is_empty())
        {
            ui.add_space(100.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Твоя коллекция на всех устройствах")
                        .size(25.0)
                        .strong(),
                );
                ui.label("Войди, чтобы сохранять избранное и собирать альбомы.");
                ui.add_space(16.0);
                if self
                    .primary(ui, "Войти или создать аккаунт", !self.cloud_busy)
                    .clicked()
                {
                    self.account_open = true;
                }
                if self.cloud_error {
                    ui.label(&self.cloud_status);
                }
            });
            return;
        }
        if self.cloud_error {
            ui.label(RichText::new(&self.cloud_status).color(if self.dark() {
                Color32::LIGHT_RED
            } else {
                Color32::DARK_RED
            }));
        }
        if self.view == View::Albums {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.album_name)
                        .hint_text("Название нового альбома")
                        .char_limit(80),
                );
                if ui
                    .add_enabled(
                        !self.cloud_busy && !self.album_name.trim().is_empty(),
                        egui::Button::new("Создать альбом"),
                    )
                    .clicked()
                {
                    let name = std::mem::take(&mut self.album_name);
                    self.cloud_send(CloudJob::Op(
                        serde_json::json!({"op":"album_create","name":name}),
                    ));
                }
            });
            ui.add_space(8.0);
            let albums = self.cloud_data.albums.clone();
            ui.horizontal_wrapped(|ui| {
                for a in albums {
                    if ui
                        .selectable_label(
                            self.album_selected.as_ref() == Some(&a.id),
                            format!("{} · {}", a.name, a.items.len()),
                        )
                        .clicked()
                    {
                        self.album_selected = Some(a.id);
                        self.album_rename = a.name;
                        self.album_confirm_delete = false;
                    }
                }
            });
            if let Some(id) = self.album_selected.clone() {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui|{
                    ui.add(egui::TextEdit::singleline(&mut self.album_rename).char_limit(80).desired_width(220.0));
                    if ui.add_enabled(!self.cloud_busy&&!self.album_rename.trim().is_empty(),egui::Button::new("Переименовать")).clicked(){self.cloud_send(CloudJob::Op(serde_json::json!({"op":"album_rename","id":id,"name":self.album_rename})));}
                    if !self.album_confirm_delete{if ui.button("Удалить альбом…").clicked(){self.album_confirm_delete=true;}}
                    else{ui.label("Избранное останется.");if ui.add_enabled(!self.cloud_busy,egui::Button::new("Подтвердить удаление")).clicked(){self.cloud_send(CloudJob::Op(serde_json::json!({"op":"album_delete","id":id})));self.album_confirm_delete=false;}if ui.button("Отмена").clicked(){self.album_confirm_delete=false;}}
                });
            } else {
                ui.add_space(24.0);
                ui.label(if self.cloud_data.albums.is_empty() {
                    "Создай первый альбом. Затем добавь изображения из избранного."
                } else {
                    "Выбери альбом выше."
                });
                return;
            }
        }
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.gallery_search)
                    .hint_text("Поиск по автору или категории…")
                    .desired_width(260.0),
            );
            ui.add(
                egui::Slider::new(&mut self.gallery_tile_width, 160.0..=320.0)
                    .text("Размер")
                    .show_value(false),
            );
        });
        ui.add_space(12.0);
        let membership: HashSet<_> = self
            .album_selected
            .as_ref()
            .and_then(|id| self.cloud_data.albums.iter().find(|a| &a.id == id))
            .map(|a| a.items.iter().cloned().collect())
            .unwrap_or_default();
        let mut entries: Vec<_> = self
            .cloud_data
            .favorites
            .iter()
            .rev()
            .filter(|p| self.view != View::Albums || membership.contains(&p.key))
            .cloned()
            .map(GalleryEntry::Remote)
            .collect();
        if self.view == View::Favorites {
            entries.extend(
                self.settings
                    .local_favorites
                    .iter()
                    .rev()
                    .cloned()
                    .map(GalleryEntry::Local),
            );
        }
        let search = self.gallery_search.to_lowercase();
        entries.retain(|e| match e {
            GalleryEntry::Local(p) => p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
                .contains(&search),
            GalleryEntry::Remote(m) => format!("{} {} {}", m.artist, m.tag, tag_label(&m.tag))
                .to_lowercase()
                .contains(&search),
        });
        self.gallery_grid(ui, entries, "favorites-grid");
    }
}
fn tag_label(tag: &str) -> String {
    match tag {
        "waifu" => "Вайфу",
        "maid" => "Горничные",
        "uniform" => "Униформа",
        "marin-kitagawa" => "Марин Китагава",
        "mori-calliope" => "Мори Каллиопа",
        "raiden-shogun" => "Райдэн",
        "oppai" => "Пышные формы",
        "selfies" => "Селфи",
        "oral" => "Оральный секс",
        "hentai" => "Хентай",
        "ass" => "Ягодицы",
        "ero" => "Эротика",
        "ecchi" => "Этти",
        _ => return tag.replace('-', " "),
    }
    .into()
}

impl GalleryApp {
    fn toggle_local_favorite(&mut self, path: PathBuf) {
        if self.settings.local_favorites.contains(&path) {
            self.settings.local_favorites.retain(|p| p != &path);
            self.message("Убрано из локального избранного", false);
        } else {
            self.settings.local_favorites.push(path);
            self.message("Добавлено в избранное на этом компьютере. У этого старого файла нет исходной ссылки для синхронизации.",false);
        }
        self.changed();
        self.queue_favorite_thumbnails();
    }
}
