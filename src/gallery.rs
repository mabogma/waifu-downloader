use crate::{api::Metadata, assets::Loaded};
use eframe::egui::TextureHandle;
use std::{collections::{HashSet, VecDeque}, path::PathBuf};
#[derive(Clone)]
pub enum Entry {
    Local(PathBuf),
    Remote(Metadata),
}
impl Entry {
    pub fn key(&self) -> String {
        match self {
            Self::Local(p) => p.to_string_lossy().into(),
            Self::Remote(m) => m.key.clone(),
        }
    }
}
pub struct Viewer {
    pub session: u64,
    pub pending: HashSet<String>,
    pub prefetch_seen: HashSet<String>,
    pub entries: Vec<Entry>,
    pub index: usize,
    pub requested_index: usize,
    pub epoch: u64,
    pub busy: bool,
    pub texture: Option<TextureHandle>,
    pub error: Option<String>,
    pub zoom: f32,
    pub cache: VecDeque<(String, Loaded)>,
}
impl Viewer {
    pub fn new(entries: Vec<Entry>, index: usize) -> Self {
        Self {
            session: 0,
            pending: HashSet::new(),
            prefetch_seen: HashSet::new(),
            entries,
            index,
            requested_index: index,
            epoch: 0,
            busy: false,
            texture: None,
            error: None,
            zoom: 1.0,
            cache: VecDeque::new(),
        }
    }
    pub fn step(&mut self, delta: isize) {
        self.index = (self.index as isize + delta).rem_euclid(self.entries.len() as isize) as usize;
        self.prefetch_seen.clear();
        self.texture = None;
        self.error = None;
        self.zoom = 1.0;
    }
    pub fn loaded(&self) -> Option<&Loaded> {
        let key = self.entries[self.index].key();
        self.cache.iter().find(|(k, _)| k == &key).map(|(_, l)| l)
    }
    pub fn neighbors(&self) -> Vec<usize> {
        let n=self.entries.len();if n<2{return vec![];}
        let next=(self.index+1)%n;let previous=(self.index+n-1)%n;
        if next==previous{vec![next]}else{vec![next,previous]}
    }
    pub fn insert(&mut self, key: String, loaded: Loaded) {
        self.cache.retain(|(k, _)| k != &key);
        self.cache.push_front((key, loaded));
        while self.cache.len() > 1
            && (self.cache.len() > 3
                || self
                    .cache
                    .iter()
                    .map(|(_, l)| {
                        l.source.memory_bytes()
                            + l.preview.pixels.len() * 4
                            + l.thumbnail.pixels.len() * 4
                    })
                    .sum::<usize>()
                    > 64 * 1024 * 1024)
        {
            let current=self.entries[self.index].key();
            if let Some(index)=self.cache.iter().rposition(|(key,_)|key!=&current){self.cache.remove(index);}else{break;}
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefetch_neighbors_are_unique_and_wrap() {
        let mut g=Viewer::new(vec![Entry::Local("a".into())],0);
        assert!(g.neighbors().is_empty());
        g.entries.push(Entry::Local("b".into()));
        assert_eq!(g.neighbors(),vec![1]);
        g.entries.push(Entry::Local("c".into()));
        assert_eq!(g.neighbors(),vec![1,2]);
        g.step(-1);
        assert_eq!(g.neighbors(),vec![0,1]);
    }
    #[test]
    fn prefetch_keeps_current_image_and_limits_cache() {
        let entries=(0..5).map(|i|Entry::Local(format!("{i}").into())).collect();
        let mut g=Viewer::new(entries,0);
        for i in 0..5 {
            let key=i.to_string();
            g.insert(key.clone(),Loaded{
                source:crate::assets::Source::Disk(key.clone().into()),
                meta:Metadata{key,source:None,artist:String::new(),mode:"SFW".into(),tag:"waifu".into()},
                dimensions:[1,1],
                preview:eframe::egui::ColorImage::new([1,1],eframe::egui::Color32::WHITE),
                thumbnail:eframe::egui::ColorImage::new([1,1],eframe::egui::Color32::WHITE),
            });
        }
        assert!(g.loaded().is_some());assert_eq!(g.cache.len(),3);
        g.prefetch_seen.insert("1".into());g.step(1);assert!(g.prefetch_seen.is_empty());
    }
    #[test]
    fn browsing_wraps_without_changing_the_list() {
        let entries = vec![
            Entry::Local("a.png".into()),
            Entry::Local("b.png".into()),
            Entry::Local("c.png".into()),
        ];
        let mut g = Viewer::new(entries, 0);
        g.step(-1);
        assert_eq!(g.index, 2);
        g.step(1);
        assert_eq!(g.index, 0);
        g.step(1);
        assert_eq!(g.index, 1);
        assert_eq!(g.entries[0].key(), "a.png");
    }
}
