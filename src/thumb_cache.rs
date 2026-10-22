use crate::{api::{Api,Metadata},assets::{self,Source}};
use eframe::egui::ColorImage;
use sha2::{Digest,Sha256};
use std::{path::{Path,PathBuf},sync::Arc};

pub fn load(api:Result<&Api,String>,meta:Metadata,local:Option<PathBuf>,cache:Option<&Path>)->Result<ColorImage,String>{
    let path=cache.map(|folder|folder.join(format!("{:x}.png",Sha256::digest(meta.key.as_bytes()))));
    if let Some(path)=&path {if let Ok(image)=assets::thumbnail(path){return Ok(image);}}
    let image=if let Some(local)=local.filter(|p|p.is_file()) {assets::thumbnail(&local)?}
    else {let download=api?.direct(meta)?;assets::load(Source::Memory(Arc::new(download.bytes)),download.meta)?.thumbnail};
    if let Some(path)=path {
        if std::fs::create_dir_all(path.parent().unwrap()).is_ok(){
            let temp=path.with_extension(format!("{}.tmp",std::process::id()));
            let rgba:Vec<u8>=image.pixels.iter().flat_map(|p|p.to_array()).collect();
            if image::save_buffer_with_format(&temp,&rgba,image.width() as u32,image.height() as u32,image::ColorType::Rgba8,image::ImageFormat::Png).is_ok(){let _=std::fs::rename(&temp,&path);}
            let _=std::fs::remove_file(temp);
            prune(path.parent().unwrap());
        }
    }
    Ok(image)
}
fn prune(folder:&Path){
    let Ok(files)=std::fs::read_dir(folder) else{return;};
    let mut files:Vec<_>=files.flatten().filter_map(|e|{let p=e.path();if p.extension().is_some_and(|s|s=="png"){e.metadata().ok().map(|m|(p,m.len(),m.modified().ok()))}else{None}}).collect();
    let mut bytes=files.iter().map(|(_,len,_)|len).sum::<u64>();let mut count=files.len();
    if count<=256&&bytes<=32*1024*1024{return;}
    files.sort_by_key(|(_,_,time)|*time);
    for (path,len,_) in files {if count<=256&&bytes<=32*1024*1024{break;}if std::fs::remove_file(path).is_ok(){count-=1;bytes=bytes.saturating_sub(len);}}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn cached_favorite_thumbnail_works_without_network_or_original(){
        let folder=std::env::temp_dir().join(format!("waifu-thumb-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&folder).unwrap();let original=folder.join("original.png");image::DynamicImage::new_rgb8(240,160).save(&original).unwrap();
        let meta=Metadata{key:"https://cdn.example/test".into(),source:None,artist:String::new(),mode:"SFW".into(),tag:"waifu".into()};
        let cache=folder.join("cache");let first=load(Err("offline".into()),meta.clone(),Some(original.clone()),Some(&cache)).unwrap();
        std::fs::remove_file(original).unwrap();let restored=load(Err("offline".into()),meta,None,Some(&cache)).unwrap();assert_eq!(first.size,restored.size);
        for entry in std::fs::read_dir(&cache).unwrap().flatten(){std::fs::remove_file(entry.path()).unwrap();}std::fs::remove_dir(cache).unwrap();std::fs::remove_dir(folder).unwrap();
    }
}
