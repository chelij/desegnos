import { deflateSync, crc32 } from 'node:zlib';

// Valid, deliberately incompressible RGB pictures: about 270 KB apiece.
export function codeMetaFixture() {
  const chunk = (name, data) => {
    const type = Buffer.from(name), size = Buffer.alloc(4), crc = Buffer.alloc(4);
    size.writeUInt32BE(data.length); crc.writeUInt32BE(crc32(Buffer.concat([type, data])));
    return Buffer.concat([size, type, data, crc]);
  };
  const pictures = Array.from({ length: 6 }, (_, seed) => {
    const header = Buffer.alloc(13); header.writeUInt32BE(300); header.writeUInt32BE(300, 4); header[8] = 8; header[9] = 2;
    const pixels = Buffer.alloc(300 * 901); let random = seed + 1;
    for (let y = 0; y < 300; y++) for (let x = 1; x <= 900; x++) {
      random ^= random << 13; random ^= random >>> 17; random ^= random << 5;
      pixels[y * 901 + x] = random & 255;
    }
    return 'data:image/png;base64,' + Buffer.concat([Buffer.from('89504e470d0a1a0a', 'hex'), chunk('IHDR', header), chunk('IDAT', deflateSync(pixels)), chunk('IEND', Buffer.alloc(0))]).toString('base64');
  });
  const html = '<main data-desegnos-id="fixture-main" data-desegnos-region="fixture-region">\n' + pictures.map((uri, i) =>
    `<section data-desegnos-id="fixture-section-${i}"><img data-desegnos-id="fixture-img-${i}" src="${uri}" alt="Picture ${i}">\n` +
    Array.from({ length: 15 }, (_, j) => `<p data-desegnos-id="fixture-p-${i}-${j}" class="copy">A real site has navigation, stories, photographs and useful information. This paragraph keeps the source at a realistic size.</p>`).join('\n') + '\n</section>').join('\n') + '\n</main>';
  let id = 0;
  const source = html.replace(/data-desegnos-id="[^"]+"/g, () => `data-desegnos-id="${String(++id).padStart(12, '0')}"`).replace('fixture-region', '00000000-0000-4000-8000-999999999999');
  return { pictures, html: source, css: 'img{width:120px;height:120px}main{padding:20px}', javascript: "document.querySelector('main').dataset.ready = 'yes';" };
}
