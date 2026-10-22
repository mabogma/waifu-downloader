use crate::{
    api::{Api, Metadata, Query},
    assets::{self, Loaded, Source},
    settings::Settings,
    system,
};
use eframe::egui::{ColorImage, Context};
use std::{
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, Sender, SyncSender},
        Arc,
    },
};

pub enum NetJob {
    SourceTags(crate::sources::SourceProfile, String),
    Gallery(u64, Metadata),
    Tags,
    Image(u64, Query),
}
pub enum DiskJob {
    Gallery(u64, PathBuf),
    Open(u64, Source, Metadata),
    Thumb(PathBuf),
    Scan(u64, Vec<PathBuf>, Vec<PathBuf>),
    SaveWithMetadata(Metadata, Source, PathBuf),
    Settings(Settings, PathBuf),
    Appearance,
    Flush(Sender<()>),
}
pub enum Event {
    GalleryPrefetch(u64, String, Result<Box<Loaded>,String>),
    SavedIndex(u64, std::collections::HashMap<String, Vec<PathBuf>>),
    SourceTags(String, String, Result<Vec<String>,String>),
    Gallery(u64, Result<Box<Loaded>, String>),
    RemoteThumb(String, Result<ColorImage, String>),
    Tags(Result<Vec<String>, String>),
    Image(u64, Result<Box<Loaded>, String>),
    Open(u64, Result<Box<Loaded>, String>),
    Thumb(PathBuf, Result<ColorImage, String>),
    Scan(u64, Result<Vec<PathBuf>, String>),
    Saved(String, Result<PathBuf, String>),
    Settings(Result<(), String>),
    Appearance(system::Appearance),
}
pub struct Workers {
    pub gallery_prefetch: SyncSender<(u64,crate::gallery::Entry)>,
    pub prefetch_generation: Arc<std::sync::atomic::AtomicU64>,
    pub remote_thumbs: SyncSender<(Metadata,Option<PathBuf>)>,
    pub net: SyncSender<NetJob>,
    pub disk: SyncSender<DiskJob>,
    pub events: Receiver<Event>,
}
fn emit(tx: &Sender<Event>, ctx: &Context, event: Event) {
    if tx.send(event).is_ok() {
        ctx.request_repaint();
    }
}
impl Workers {
    #[cfg(test)]
    pub fn start(ctx:Context)->Self {Self::start_with_cache(ctx,None)}
    pub fn start_with_cache(ctx: Context,cache:Option<PathBuf>) -> Self {
        let (tx, events) = mpsc::channel();
        let (net_tx, net_rx) = mpsc::sync_channel(2);
        let (disk_tx, disk_rx) = mpsc::sync_channel(24);
        let (thumb_tx,thumb_rx)=mpsc::sync_channel::<(Metadata,Option<PathBuf>)>(24);
        let thumb_rx=Arc::new(std::sync::Mutex::new(thumb_rx));
        for _ in 0..3 {
            let rx=thumb_rx.clone();let tx=tx.clone();let ctx=ctx.clone();let cache=cache.clone();
            std::thread::spawn(move || {
                let api=Api::new();
                loop {
                    let job={rx.lock().unwrap().recv()};let Ok((meta,local))=job else{break;};
                    let key=meta.key.clone();
                    let result=crate::thumb_cache::load(api.as_ref().map_err(Clone::clone),meta,local,cache.as_deref());
                    emit(&tx,&ctx,Event::RemoteThumb(key,result));
                }
            });
        }
        let (gallery_prefetch,prefetch_rx)=mpsc::sync_channel::<(u64,crate::gallery::Entry)>(1);
        let prefetch_generation=Arc::new(std::sync::atomic::AtomicU64::new(0));
        let generation=prefetch_generation.clone();let pt=tx.clone();let pc=ctx.clone();
        std::thread::spawn(move || {
            let api=Api::new();
            while let Ok((session,entry))=prefetch_rx.recv(){
                if generation.load(std::sync::atomic::Ordering::Relaxed)!=session{continue;}
                let key=entry.key();
                let result=match entry {
                    crate::gallery::Entry::Local(path)=>assets::load(Source::Disk(path.clone()),assets::disk_metadata(&path)),
                    crate::gallery::Entry::Remote(meta)=>api.as_ref().map_err(Clone::clone).and_then(|a|a.direct(meta)).and_then(|d|assets::load(Source::Memory(Arc::new(d.bytes)),d.meta)),
                }.map(Box::new);
                if generation.load(std::sync::atomic::Ordering::Relaxed)==session {emit(&pt,&pc,Event::GalleryPrefetch(session,key,result));}
            }
        });
        let nt = tx.clone();
        let nc = ctx.clone();
        std::thread::spawn(move || {
            let api = Api::new();
            while let Ok(job) = net_rx.recv() {
                let event = match job {
                    NetJob::SourceTags(profile, mode) => {
                        let tx=nt.clone();let ctx=nc.clone();
                        std::thread::spawn(move || {let result=crate::sources::discover(&profile,&mode);emit(&tx,&ctx,Event::SourceTags(profile.url,mode,result));});
                        continue;
                    },
                    NetJob::Gallery(epoch, meta) => Event::Gallery(
                        epoch,
                        api.as_ref().map_err(Clone::clone).and_then(|api| {
                            let d = api.direct(meta)?;
                            assets::load(Source::Memory(Arc::new(d.bytes)), d.meta).map(Box::new)
                        }),
                    ),
                    NetJob::Tags => {
                        Event::Tags(api.as_ref().map_err(Clone::clone).and_then(Api::tags))
                    }
                    NetJob::Image(epoch, q) => Event::Image(
                        epoch,
                        api.as_ref().map_err(Clone::clone).and_then(|api| {
                            let d = api.image(&q)?;
                            assets::load(Source::Memory(Arc::new(d.bytes)), d.meta).map(Box::new)
                        }),
                    ),
                };
                emit(&nt, &nc, event);
            }
        });
        std::thread::spawn(move || {
            let mut saved_index:std::collections::HashMap<String,Vec<PathBuf>>=Default::default();
            while let Ok(job) = disk_rx.recv() {
                let event = match job {
                    DiskJob::Gallery(epoch, path) => Event::Gallery(
                        epoch,
                        assets::load(Source::Disk(path.clone()), assets::disk_metadata(&path))
                            .map(Box::new),
                    ),
                    DiskJob::SaveWithMetadata(meta, source, folder) => {
                        let existing=saved_index.get(&meta.key).and_then(|paths|paths.iter().find(|p|p.is_file())).cloned();
                        let result = if let Some(path)=existing {Ok(path)} else {assets::save_png(&source, &folder)};
                        if let Ok(path) = &result {
                            let paths=saved_index.entry(meta.key.clone()).or_default();if !paths.contains(path){paths.push(path.clone());}
                            if let Err(e) = assets::save_metadata(path, &meta) {
                                emit(
                                    &tx,
                                    &ctx,
                                    Event::Settings(Err(format!(
                                        "Исходная ссылка не сохранена: {e}"
                                    ))),
                                );
                            }
                        }
                        Event::Saved(meta.key, result)
                    }
                    DiskJob::Open(epoch, source, mut meta) => {
                        if let Source::Disk(path) = &source {
                            meta = assets::disk_metadata(path);
                        }
                        Event::Open(epoch, assets::load(source, meta).map(Box::new))
                    }
                    DiskJob::Thumb(path) => Event::Thumb(path.clone(), assets::thumbnail(&path)),
                    DiskJob::Scan(epoch, roots, all_roots) => {
                        if let Ok(paths)=assets::scan(all_roots) {
                            saved_index.clear();
                            for path in paths {saved_index.entry(assets::disk_metadata(&path).key).or_default().push(path);}
                            emit(&tx,&ctx,Event::SavedIndex(epoch,saved_index.clone()));
                        }
                        Event::Scan(epoch, assets::scan(roots))
                    },
                    DiskJob::Settings(settings, base) => Event::Settings(settings.save(&base)),
                    DiskJob::Appearance => Event::Appearance(system::read()),
                    DiskJob::Flush(done) => {
                        let _ = done.send(());
                        continue;
                    }
                };
                emit(&tx, &ctx, event);
            }
        });
        Self {
            gallery_prefetch,
            prefetch_generation,
            remote_thumbs: thumb_tx,
            net: net_tx,
            disk: disk_tx,
            events,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_worker_finishes_save_before_flush() {
        use std::io::Cursor;
        use std::time::{Duration, SystemTime, UNIX_EPOCH};
        let folder = std::env::temp_dir().join(format!(
            "waifu-worker-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut encoded = Cursor::new(vec![]);
        image::DynamicImage::new_rgb8(240, 160)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let workers = Workers::start(Context::default());
        workers
            .disk
            .send(DiskJob::SaveWithMetadata(
                Metadata {
                    key: "https://example.com/key.png".into(),
                    source: None,
                    artist: String::new(),
                    mode: "SFW".into(),
                    tag: "waifu".into(),
                },
                Source::Memory(Arc::new(encoded.into_inner())),
                folder.clone(),
            ))
            .unwrap();
        let (tx, rx) = mpsc::channel();
        workers.disk.send(DiskJob::Flush(tx)).unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let paths = assets::scan(vec![folder.clone()]).unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(image::image_dimensions(&paths[0]).unwrap(), (240, 160));
        assert!(
            matches!(workers.events.try_recv().unwrap(),Event::Saved(key,Ok(_)) if key=="https://example.com/key.png")
        );
        assert_eq!(
            assets::disk_metadata(&paths[0]).key,
            "https://example.com/key.png"
        );
        // A second request with the same source must reuse the existing file,
        // even after rebuilding the index as happens on application restart.
        workers.disk.send(DiskJob::Scan(1,vec![folder.clone()],vec![folder.clone()])).unwrap();
        workers.disk.send(DiskJob::SaveWithMetadata(assets::disk_metadata(&paths[0]),Source::Memory(Arc::new(vec![])),folder.clone())).unwrap();
        let (tx,rx)=mpsc::channel();workers.disk.send(DiskJob::Flush(tx)).unwrap();rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(assets::scan(vec![folder.clone()]).unwrap(),paths);
        let mut reused=false;
        while let Ok(event)=workers.events.try_recv(){if let Event::Saved(_,result)=event{assert_eq!(result.unwrap(),paths[0]);reused=true;}}
        assert!(reused);
        std::fs::remove_file(paths[0].with_extension("waifu.json")).unwrap();
        std::fs::remove_file(&paths[0]).unwrap();
        std::fs::remove_dir(folder).unwrap();
    }
}
