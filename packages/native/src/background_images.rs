//! The images behind `url()` in `background` and `backgroundImage`.
//!
//! Parsing has no window, so it only names an image: the layer carries
//! `ImageKey` (a hash of the address) and `request` queues the address on a
//! thread-local. `pump` runs where a window is at hand (`build_element`, which
//! also covers a virtual-list row built in list layout) and moves each queued
//! address through the [`Store`], an App global: the test renderer opens a new
//! App per window, so a thread-local store would outlive the table of images
//! gpui paints from (`BackgroundImages`, also per App).
//!
//! An address resolves exactly as an `<img>` `src` does (`img::resolve_src`):
//! a path or `file://` URL and `http(s)` go through gpui's `ImageAssetLoader`,
//! which re-renders the view when the load ends, `data:` is decoded once, and
//! `blob:` waits for the bytes the JS layer hands to `deliver`
//! (`js/style-images.ts`). A layer paints nothing until its image is in the
//! table, and nothing if the load failed.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use gpui::{App, BackgroundImages, Global, ImageKey, RenderImage, Window};

use crate::custom_elements::img::{self, ImgSource};

/// Images kept at once. Past it the ones requested longest ago are dropped,
/// from the table and the sprite atlas, and loaded again if a layer asks.
const MAX_IMAGES: usize = 128;

/// The key a layer refers to its image by. `DefaultHasher::new()` has fixed
/// keys, so the same address is the same key in every process.
pub(crate) fn key(url: &str) -> ImageKey {
    let mut hasher = DefaultHasher::new();
    url.trim().hash(&mut hasher);
    ImageKey(hasher.finish())
}

#[derive(Default)]
struct Requests {
    /// The address of every key asked for, so a build that asks again every
    /// frame copies nothing.
    urls: HashMap<ImageKey, Arc<str>>,
    pending: Vec<ImageKey>,
    queued: HashSet<ImageKey>,
}

thread_local! {
    static REQUESTS: RefCell<Requests> = RefCell::new(Requests::default());
    static HAS_PENDING: Cell<bool> = const { Cell::new(false) };
    static STILL_LOADING: Cell<u64> = const { Cell::new(0) };
}

/// How many times `pump` found an image it asked the loader for not ready yet. A build whose count
/// moved between its start and its end painted something that its recording would not show once the
/// image is there, and gpui, which learns of a pending asset from the build it runs itself, does not
/// know to refuse the recording of a build made before it.
pub(crate) fn still_loading() -> u64 {
    STILL_LOADING.get()
}

/// Asks for the image at `url` to be loaded by the next `pump`. A layer calls
/// this on every build, which is also what keeps its image from being evicted.
pub(crate) fn request(key: ImageKey, url: &str) {
    crate::tape::note_build(crate::tape::BuildEffect::Image(key, url.into()));
    REQUESTS.with_borrow_mut(|requests| {
        if requests.queued.insert(key) {
            requests.urls.entry(key).or_insert_with(|| Arc::from(url.trim()));
            requests.pending.push(key);
        }
    });
    HAS_PENDING.set(true);
}

fn take_requests() -> Vec<(ImageKey, Arc<str>)> {
    REQUESTS.with_borrow_mut(|requests| {
        requests.queued.clear();
        let pending = std::mem::take(&mut requests.pending);
        pending
            .into_iter()
            .map(|key| (key, requests.urls[&key].clone()))
            .collect()
    })
}

/// The addresses requested on this thread since the last take, for tests of the
/// parsers that ask for them.
#[cfg(test)]
pub(crate) fn requested_for_tests() -> Vec<String> {
    take_requests().into_iter().map(|(_, url)| url.to_string()).collect()
}

fn forget_url(key: ImageKey) {
    REQUESTS.with_borrow_mut(|requests| {
        if !requests.queued.contains(&key) {
            requests.urls.remove(&key);
        }
    });
}

#[derive(Debug)]
enum State {
    Loading,
    Loaded(Arc<RenderImage>),
    Failed(String),
}

struct Entry {
    url: Arc<str>,
    source: ImgSource,
    state: State,
    /// When it was last requested: the Store's clock at that moment.
    stamp: u64,
}

/// What `pump` does for an entry that is still loading.
enum Step {
    Idle,
    Asset(gpui::Resource),
    Decode(Arc<gpui::Image>),
}

/// The state of every image a layer has asked for, per App.
#[derive(Default)]
struct Store {
    entries: HashMap<ImageKey, Entry>,
    clock: u64,
}

impl Global for Store {}

impl Store {
    /// Records that `key` was asked for, and says what moves it forward.
    fn touch(&mut self, key: ImageKey, url: &Arc<str>) -> Step {
        self.clock += 1;
        let stamp = self.clock;
        let entry = self.entries.entry(key).or_insert_with(|| Entry::new(url));
        entry.stamp = stamp;
        if !matches!(entry.state, State::Loading) {
            return Step::Idle;
        }
        match &entry.source {
            ImgSource::Path(path) => Step::Asset(gpui::Resource::Path(path.as_path().into())),
            ImgSource::Uri(uri) => Step::Asset(gpui::Resource::Uri(uri.clone())),
            ImgSource::Data(image) => Step::Decode(image.clone()),
            _ => Step::Idle,
        }
    }

    /// Ends a load. Only an image that is still loading takes an outcome, and
    /// the image of a success is returned for the table.
    fn settle(&mut self, key: ImageKey, outcome: Result<Arc<RenderImage>, String>) -> Option<Arc<RenderImage>> {
        let entry = self.entries.get_mut(&key)?;
        if !matches!(entry.state, State::Loading) {
            return None;
        }
        match outcome {
            Ok(image) => {
                entry.state = State::Loaded(image.clone());
                Some(image)
            }
            Err(message) => {
                log::warn!("background image {}: {message}", entry.url);
                entry.state = State::Failed(message);
                None
            }
        }
    }

    /// Forgets the images requested longest ago until `cap` are left, and
    /// returns what was forgotten with the pixels of the ones that had loaded.
    fn evict(&mut self, cap: usize) -> Vec<(ImageKey, Option<Arc<RenderImage>>)> {
        let mut evicted = Vec::new();
        while self.entries.len() > cap {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.stamp)
                .map(|(key, _)| *key)
            else {
                break;
            };
            let entry = self.entries.remove(&oldest).expect("the key was just found");
            let image = match entry.state {
                State::Loaded(image) => Some(image),
                _ => None,
            };
            evicted.push((oldest, image));
        }
        evicted
    }

    /// The bytes of a `blob:` image have arrived, or its fetch failed. Errors
    /// when `url` is not a `blob:` address, which has no other way to load.
    fn expects_bytes(&mut self, key: ImageKey, url: &Arc<str>) -> Result<bool, String> {
        self.touch(key, url);
        let entry = &self.entries[&key];
        if !matches!(entry.source, ImgSource::Pending) {
            return Err(format!("{url} is not a blob: URL"));
        }
        Ok(matches!(entry.state, State::Loading))
    }
}

impl Entry {
    fn new(url: &Arc<str>) -> Self {
        let source = img::resolve_src(url);
        let state = match &source {
            ImgSource::Failed(message) => {
                log::warn!("background image {url}: {message}");
                State::Failed(message.clone())
            }
            _ => State::Loading,
        };
        Self { url: url.clone(), source, state, stamp: 0 }
    }
}

/// Moves every requested image forward. Cheap when nothing was requested.
pub(crate) fn pump(window: &mut Window, cx: &mut App) {
    if !HAS_PENDING.get() {
        return;
    }
    HAS_PENDING.set(false);
    for (key, url) in take_requests() {
        let step = cx.default_global::<Store>().touch(key, &url);
        let outcome = match step {
            Step::Idle => continue,
            Step::Asset(resource) => match window.use_asset::<gpui::ImageAssetLoader>(&resource, cx) {
                Some(result) => result.map_err(|error| error.to_string()),
                None => {
                    STILL_LOADING.set(STILL_LOADING.get() + 1);
                    continue;
                }
            },
            Step::Decode(image) => {
                let renderer = cx.svg_renderer();
                image.to_image_data(renderer).map_err(|error| error.to_string())
            }
        };
        settle(cx, key, outcome);
    }
    evict(window, cx);
}

fn settle(cx: &mut App, key: ImageKey, outcome: Result<Arc<RenderImage>, String>) {
    if let Some(image) = cx.default_global::<Store>().settle(key, outcome) {
        BackgroundImages::set(cx, key, image);
    }
}

fn evict(window: &mut Window, cx: &mut App) {
    for (key, image) in cx.default_global::<Store>().evict(MAX_IMAGES) {
        BackgroundImages::remove(cx, key);
        if let Some(image) = image {
            window.drop_image(image).ok();
        }
        forget_url(key);
    }
}

/// The bytes of the `blob:` image at `url`, from the JS layer.
pub(crate) fn deliver(
    url: &str,
    bytes: Vec<u8>,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), String> {
    let url: Arc<str> = Arc::from(url.trim());
    let key = key(&url);
    let result = if cx.default_global::<Store>().expects_bytes(key, &url)? {
        let outcome = img::render_image_from_encoded(bytes, cx.svg_renderer());
        let failure = outcome.as_ref().err().cloned();
        settle(cx, key, outcome);
        failure.map_or(Ok(()), Err)
    } else {
        Ok(())
    };
    evict(window, cx);
    result
}

/// The JS layer could not read the `blob:` image at `url`.
pub(crate) fn fail(url: &str, message: String, window: &mut Window, cx: &mut App) -> Result<(), String> {
    let url: Arc<str> = Arc::from(url.trim());
    let key = key(&url);
    if cx.default_global::<Store>().expects_bytes(key, &url)? {
        settle(cx, key, Err(message));
    }
    evict(window, cx);
    Ok(())
}

/// [`deliver`] from a call on the view, which has to draw what the image changes.
pub(crate) fn deliver_to_view(
    url: &str,
    bytes: Vec<u8>,
    window: &mut Window,
    cx: &mut gpui::Context<crate::renderer::GpuixView>,
) -> Result<(), String> {
    let result = deliver(url, bytes, window, cx);
    cx.notify();
    window.refresh();
    result
}

/// [`fail`] from a call on the view.
pub(crate) fn fail_in_view(
    url: &str,
    message: String,
    window: &mut Window,
    cx: &mut gpui::Context<crate::renderer::GpuixView>,
) -> Result<(), String> {
    let result = fail(url, message, window, cx);
    cx.notify();
    window.refresh();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom_elements::img::PixelFormat;

    fn image() -> Arc<RenderImage> {
        img::render_image_from_pixels(1, 1, vec![0; 4], PixelFormat::Rgba).unwrap()
    }

    fn drain() -> Vec<(ImageKey, Arc<str>)> {
        take_requests()
    }

    fn state<'a>(store: &'a Store, key: ImageKey) -> &'a State {
        &store.entries[&key].state
    }

    #[test]
    fn the_key_hashes_the_trimmed_address() {
        assert_eq!(key("a.png"), key("  a.png\n"));
        assert_ne!(key("a.png"), key("b.png"));
        assert_eq!(key("a.png"), key("a.png"));
    }

    #[test]
    fn a_request_is_queued_once_until_it_is_taken() {
        drain();
        let a = key("queue-a.png");
        request(a, "queue-a.png");
        request(a, "queue-a.png");
        request(key("queue-b.png"), " queue-b.png ");
        assert!(HAS_PENDING.get());
        let taken = drain();
        assert_eq!(taken.len(), 2);
        assert_eq!(&*taken[0].1, "queue-a.png");
        assert_eq!(&*taken[1].1, "queue-b.png");
        assert!(drain().is_empty());

        // Asked for again, it is queued again, with the address kept from before.
        request(a, "queue-a.png");
        let again = drain();
        assert_eq!(again.len(), 1);
        assert!(Arc::ptr_eq(&again[0].1, &taken[0].1));
    }

    #[test]
    fn a_forgotten_address_is_copied_again_but_a_queued_one_is_kept() {
        drain();
        let a = key("forget-a.png");
        request(a, "forget-a.png");
        // Still queued, so the pump will need it.
        forget_url(a);
        assert_eq!(&*drain()[0].1, "forget-a.png");
        forget_url(a);
        REQUESTS.with_borrow(|requests| assert!(!requests.urls.contains_key(&a)));
        request(a, "forget-a.png");
        assert_eq!(&*drain()[0].1, "forget-a.png");
    }

    #[test]
    fn each_source_starts_loading_with_its_step() {
        let mut store = Store::default();
        let step = |store: &mut Store, url: &str| store.touch(key(url), &Arc::from(url));
        assert!(matches!(step(&mut store, "/tmp/a.png"), Step::Asset(gpui::Resource::Path(_))));
        assert!(matches!(step(&mut store, "file:///tmp/a.png"), Step::Asset(gpui::Resource::Path(_))));
        assert!(matches!(step(&mut store, "https://example.test/a.png"), Step::Asset(gpui::Resource::Uri(_))));
        assert!(matches!(step(&mut store, "data:image/svg+xml,%3Csvg%2F%3E"), Step::Decode(_)));
        assert!(matches!(step(&mut store, "blob:one"), Step::Idle));
        for url in ["/tmp/a.png", "blob:one"] {
            assert!(matches!(state(&store, key(url)), State::Loading));
        }
    }

    #[test]
    fn an_address_that_cannot_load_fails_without_a_step() {
        let mut store = Store::default();
        let bad_data = "data:image/png;base64,***";
        assert!(matches!(store.touch(key(bad_data), &Arc::from(bad_data)), Step::Idle));
        assert!(matches!(state(&store, key(bad_data)), State::Failed(message) if message == "invalid data URL"));
        let bad_file = "file://example.test/a.png";
        store.touch(key(bad_file), &Arc::from(bad_file));
        assert!(matches!(state(&store, key(bad_file)), State::Failed(_)));
    }

    #[test]
    fn only_a_loading_image_takes_an_outcome() {
        let mut store = Store::default();
        let url: Arc<str> = Arc::from("/tmp/settle.png");
        let id = key(&url);
        store.touch(id, &url);
        let loaded = image();
        assert!(Arc::ptr_eq(&store.settle(id, Ok(loaded.clone())).unwrap(), &loaded));
        assert!(matches!(state(&store, id), State::Loaded(_)));
        // Settled for good, and no longer a step.
        assert!(store.settle(id, Err("late".into())).is_none());
        assert!(store.settle(id, Ok(image())).is_none());
        assert!(matches!(state(&store, id), State::Loaded(held) if Arc::ptr_eq(held, &loaded)));
        assert!(matches!(store.touch(id, &url), Step::Idle));

        let failing: Arc<str> = Arc::from("/tmp/missing.png");
        let failing_id = key(&failing);
        store.touch(failing_id, &failing);
        assert!(store.settle(failing_id, Err("no such file".into())).is_none());
        assert!(matches!(state(&store, failing_id), State::Failed(message) if message == "no such file"));
        assert!(matches!(store.touch(failing_id, &failing), Step::Idle));
        assert!(store.settle(key("never asked"), Ok(image())).is_none());
    }

    #[test]
    fn the_images_requested_longest_ago_are_evicted_first() {
        let mut store = Store::default();
        let failing: Arc<str> = Arc::from("/tmp/fails.png");
        store.touch(key(&failing), &failing);
        store.settle(key(&failing), Err("bad".into()));
        let urls: Vec<Arc<str>> = (0..5).map(|index| Arc::from(format!("/tmp/{index}.png"))).collect();
        let loaded: Vec<_> = urls
            .iter()
            .map(|url| {
                store.touch(key(url), url);
                let image = image();
                store.settle(key(url), Ok(image.clone()));
                image
            })
            .collect();
        // The oldest image is asked for again, so the next oldest goes first.
        store.touch(key(&urls[0]), &urls[0]);
        let evicted = store.evict(3);
        let keys: Vec<_> = evicted.iter().map(|(evicted, _)| *evicted).collect();
        assert_eq!(keys, vec![key(&failing), key(&urls[1]), key(&urls[2])]);
        // A loaded image comes back so the caller can free its atlas entry.
        assert!(evicted[0].1.is_none());
        assert!(Arc::ptr_eq(evicted[2].1.as_ref().unwrap(), &loaded[2]));
        assert_eq!(store.entries.len(), 3);
        assert!(store.evict(3).is_empty());
    }

    #[test]
    fn bytes_are_only_expected_for_a_blob_that_is_still_loading() {
        let mut store = Store::default();
        let blob: Arc<str> = Arc::from("blob:nodedata:1");
        assert_eq!(store.expects_bytes(key(&blob), &blob), Ok(true));
        store.settle(key(&blob), Ok(image()));
        assert_eq!(store.expects_bytes(key(&blob), &blob), Ok(false));
        let path: Arc<str> = Arc::from("/tmp/a.png");
        assert!(store.expects_bytes(key(&path), &path).is_err());
    }
}
