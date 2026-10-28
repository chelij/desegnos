// CSS named colours and colour syntax, kept local so parsing also works outside a browser.
const namedColours = Object.fromEntries(`aliceblue:f0f8ff antiquewhite:faebd7 aqua:00ffff aquamarine:7fffd4 azure:f0ffff beige:f5f5dc bisque:ffe4c4 black:000000 blanchedalmond:ffebcd blue:0000ff blueviolet:8a2be2 brown:a52a2a burlywood:deb887 cadetblue:5f9ea0 chartreuse:7fff00 chocolate:d2691e coral:ff7f50 cornflowerblue:6495ed cornsilk:fff8dc crimson:dc143c cyan:00ffff darkblue:00008b darkcyan:008b8b darkgoldenrod:b8860b darkgray:a9a9a9 darkgreen:006400 darkgrey:a9a9a9 darkkhaki:bdb76b darkmagenta:8b008b darkolivegreen:556b2f darkorange:ff8c00 darkorchid:9932cc darkred:8b0000 darksalmon:e9967a darkseagreen:8fbc8f darkslateblue:483d8b darkslategray:2f4f4f darkslategrey:2f4f4f darkturquoise:00ced1 darkviolet:9400d3 deeppink:ff1493 deepskyblue:00bfff dimgray:696969 dimgrey:696969 dodgerblue:1e90ff firebrick:b22222 floralwhite:fffaf0 forestgreen:228b22 fuchsia:ff00ff gainsboro:dcdcdc ghostwhite:f8f8ff gold:ffd700 goldenrod:daa520 gray:808080 green:008000 greenyellow:adff2f grey:808080 honeydew:f0fff0 hotpink:ff69b4 indianred:cd5c5c indigo:4b0082 ivory:fffff0 khaki:f0e68c lavender:e6e6fa lavenderblush:fff0f5 lawn green:7cfc00 lemonchiffon:fffacd lightblue:add8e6 lightcoral:f08080 lightcyan:e0ffff lightgoldenrodyellow:fafad2 lightgray:d3d3d3 lightgreen:90ee90 lightgrey:d3d3d3 lightpink:ffb6c1 lightsalmon:ffa07a lightseagreen:20b2aa lightskyblue:87cefa lightslategray:778899 lightslategrey:778899 lightsteelblue:b0c4de lightyellow:ffffe0 lime:00ff00 limegreen:32cd32 linen:faf0e6 magenta:ff00ff maroon:800000 mediumaquamarine:66cdaa mediumblue:0000cd mediumorchid:ba55d3 mediumpurple:9370db mediumseagreen:3cb371 mediumslateblue:7b68ee mediumspringgreen:00fa9a mediumturquoise:48d1cc mediumvioletred:c71585 midnightblue:191970 mintcream:f5fffa mistyrose:ffe4e1 moccasin:ffe4b5 navajowhite:ffdead navy:000080 oldlace:fdf5e6 olive:808000 olivedrab:6b8e23 orange:ffa500 orangered:ff4500 orchid:da70d6 palegoldenrod:eee8aa palegreen:98fb98 paleturquoise:afeeee palevioletred:db7093 papayawhip:ffefd5 peachpuff:ffdab9 peru:cd853f pink:ffc0cb plum:dda0dd powderblue:b0e0e6 purple:800080 rebeccapurple:663399 red:ff0000 rosybrown:bc8f8f royalblue:4169e1 saddlebrown:8b4513 salmon:fa8072 sandybrown:f4a460 seagreen:2e8b57 seashell:fff5ee sienna:a0522d silver:c0c0c0 skyblue:87ceeb slateblue:6a5acd slategray:708090 slategrey:708090 snow:fffafa springgreen:00ff7f steelblue:4682b4 tan:d2b48c teal:008080 thistle:d8bfd8 tomato:ff6347 turquoise:40e0d0 violet:ee82ee wheat:f5deb3 white:ffffff whitesmoke:f5f5f5 yellow:ffff00 yellowgreen:9acd32 transparent:00000000`.replace('lawn green', 'lawngreen').split(' ').map(pair => pair.split(':')));
const numberToken = /^[-+]?(?:\d*\.)?\d+%?$/;
const clamp = (value: number, max = 1) => Math.max(0, Math.min(max, value));
const byte = (value: number) => Math.round(clamp(value, 255)).toString(16).padStart(2, '0');

export function colourHex(value: string, allowBareHex = true): string | null {
  const token = value.trim().toLowerCase();
  if (Object.hasOwn(namedColours, token)) return `#${namedColours[token]}`;
  // Hex colours carry their model form: lower case, #rgb expanded to #rrggbb and #rgba to #rrggbbaa.
  const hex = token.startsWith('#') ? token.slice(1) : token;
  if ((token.startsWith('#') || allowBareHex) && /^(?:[\da-f]{3,4}|[\da-f]{6}|[\da-f]{8})$/.test(hex)) {
    return `#${hex.length <= 4 ? [...hex].map(char => char + char).join('') : hex}`;
  }
  const fn = /^(rgba?|hsla?)\(([^)]+)\)$/.exec(token);
  if (!fn) return null;
  const parts = fn[2].trim().split(/[\s,/]+/);
  if (parts.length < 3 || parts.length > 4 || parts.slice(1).some(part => !numberToken.test(part))) return null;
  const alpha = parts[3] === undefined ? 1 : clamp(parseFloat(parts[3]) / (parts[3].endsWith('%') ? 100 : 1));
  let rgb: number[];
  if (fn[1].startsWith('rgb')) {
    if (!numberToken.test(parts[0])) return null;
    rgb = parts.slice(0, 3).map(part => clamp(part.endsWith('%') ? parseFloat(part) / 100 * 255 : parseFloat(part), 255));
  } else {
    const hue = /^([-+]?(?:\d*\.)?\d+)(deg|grad|rad|turn)?$/.exec(parts[0]);
    if (!hue || !parts[1].endsWith('%') || !parts[2].endsWith('%')) return null;
    const degrees = Number(hue[1]) * ({ deg: 1, grad: 0.9, rad: 180 / Math.PI, turn: 360 }[hue[2] || 'deg'] ?? 1);
    if (!Number.isFinite(degrees)) return null;
    const h = ((degrees % 360) + 360) % 360 / 60;
    const s = clamp(parseFloat(parts[1]) / 100), l = clamp(parseFloat(parts[2]) / 100);
    const c = (1 - Math.abs(2 * l - 1)) * s, x = c * (1 - Math.abs(h % 2 - 1)), m = l - c / 2;
    rgb = (h < 1 ? [c, x, 0] : h < 2 ? [x, c, 0] : h < 3 ? [0, c, x] : h < 4 ? [0, x, c] : h < 5 ? [x, 0, c] : [c, 0, x]).map(channel => (channel + m) * 255);
  }
  return `#${rgb.map(byte).join('')}${alpha < 1 ? byte(alpha * 255) : ''}`;
}
