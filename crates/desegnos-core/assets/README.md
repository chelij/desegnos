# Bundled type for direct export

`Archivo-variable.ttf` is Archivo (weight and width axes) from the Google Fonts repository
(`ofl/archivo/Archivo[wdth,wght].ttf`). It is licensed under the SIL Open Font License 1.1;
`Archivo-OFL.txt` is the notice.

Slides, images and vector output convert text to glyph outlines from this face, so exported files
never depend on a font being installed. Fonts the bundled face does not cover fall back to a system
face chosen through fontdb. The variable axes stay at their default instance except weight, which
follows each element's text weight.
