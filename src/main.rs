mod decode;
mod engine;
mod keys;
mod library;
mod model;
mod source;
use std::sync::Arc;
use source::Source;
fn main() {
    let (tx, rx) = crossbeam_channel::unbounded();
    let home = std::env::var("HOME").unwrap();
    let lib = library::Library::new("/tmp/mus-scan.db".into(), vec![format!("{home}/Music").into()], tx);
    let t = std::time::Instant::now();
    lib.scan();
    let mut ui_max = std::time::Duration::ZERO;
    loop {
        // Simulated UI frame: a loop that must never stall.
        let f = std::time::Instant::now();
        match rx.recv_timeout(std::time::Duration::from_millis(16)) {
            Ok(library::LibEvent::Done { added, total, secs, .. }) => println!("scan done: {added} added, {total} total in {secs:.1}s"),
            Ok(library::LibEvent::Loudness { done, total }) if done == total => { println!("loudness done {total} in {:.1}s", t.elapsed().as_secs_f64()); break }
            Ok(_) => {}
            Err(_) => {}
        }
        ui_max = ui_max.max(f.elapsed());
        if !lib.is_scanning() { break }
    }
    println!("longest UI loop wait {:?}", ui_max);
    let src = source::local::LocalSource::new(lib.clone()).unwrap();
    for n in ["albums", "artists", "genres", "smart:week"] { println!("{n}: {}", src.children(n, source::Page::ALL).unwrap().len()); }
    let h = src.search("night", 20).unwrap();
    println!("search night: {} artists {} albums {} tracks", h.artists.len(), h.albums.len(), h.tracks.len());
    let c = library::db::open(std::path::Path::new("/tmp/mus-scan.db")).unwrap();
    for q in ["SELECT codec||' '||sample_rate||'/'||COALESCE(bits,'-')||' '||COUNT(*) FROM tracks GROUP BY 1 ORDER BY 1", "SELECT 'gain origin '||gain_origin||': '||COUNT(*) FROM tracks GROUP BY gain_origin", "SELECT 'art '||COALESCE(substr(art,1,8),'none')||': '||COUNT(*) FROM tracks GROUP BY substr(art,1,8)='embedded', art IS NULL", "SELECT 'lyrics '||lyrics||': '||COUNT(*) FROM tracks GROUP BY lyrics", "SELECT 'loudness test '||title||' gain '||round(rg_track,2) FROM tracks WHERE album='Loudness Test' ORDER BY title"] {
        let mut st = c.prepare(q).unwrap();
        for r in st.query_map([], |r| r.get::<_, String>(0)).unwrap() { println!("  {}", r.unwrap()); }
    }
}
