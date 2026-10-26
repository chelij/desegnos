//! Controlled two-page browser fixture, compiled through the real export path.
//! Usage: cargo run -p desegnos-storage --example export_browser_fixture -- /absolute/existing/output-parent
use desegnos_core::{Appearance, Page, Project, SiteContent, Viewport, compile_website};
use desegnos_storage::write_website_export;
use uuid::Uuid;
fn main() {
    let data = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    let data2 = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAEUlEQVR4nGP4z8DwH4QZYAwAR8oH+WdZbrcAAAAASUVORK5CYII=";
    let mut p = Project::new(
        "Portable check".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Light,
    )
    .unwrap();
    p.pages[0].content=Some(SiteContent{html:format!("<html lang='en'><head><title>Portable home</title></head><body><h1 id='heading'>Home</h1><img id='photo' src='{data}' srcset='{data} 1x, {data2} 2x'><div id='background'></div><button id='change'>Change text</button><a id='team' href='/about/team?view=full#lead'>Team</a></body></html>"),css:format!("body {{color:rgb(200, 0, 0)}} #background {{width:20px;height:20px;background-image:url('{data}')}}"),javascript:"document.getElementById('change').addEventListener('click',()=>document.getElementById('heading').textContent='Changed by exported JavaScript'); document.body.dataset.scriptReady='yes';".into()});
    p.pages.push(Page{id:Uuid::new_v4(),name:"Team".into(),path:"/about/team".into(),included:true,viewport:p.viewport,appearance:p.appearance,regions:vec![],content:Some(SiteContent{html:"<html><head><title>Portable team</title></head><body><h1 id='lead'>Team</h1><a id='home' href='/'>Home</a></body></html>".into(),css:format!("body {{color:rgb(0, 0, 200);background-image:url('{data}')}}"),javascript:"document.body.dataset.scriptReady='yes';".into()})});
    let compiled = compile_website(&p).unwrap();
    let parent = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("Provide an absolute existing output parent folder"),
    );
    assert!(
        parent.is_absolute() && parent.is_dir(),
        "Provide an absolute existing output parent folder"
    );
    let written = write_website_export(&parent, &p.name, &compiled).unwrap();
    println!(
        "{}",
        serde_json::json!({"path":written.path,"pages":written.page_count,"files":written.file_count,"warnings":written.warnings})
    );
}
