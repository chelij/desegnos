pub fn without_editor_ids(html: &str) -> String {
    let mut value = html.to_owned();
    while let Some(start) = value.find(" data-desegnos-id=\"") {
        let end = value[start + 19..]
            .find('"')
            .expect("editor attribute closing quote")
            + start
            + 20;
        value.replace_range(start..end, "");
    }
    value
}
