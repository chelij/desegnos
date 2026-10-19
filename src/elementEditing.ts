export function translatedStyle(inlineStyle: string, translate: string, x: number, y: number): string {
  if (![x, y].every(Number.isFinite)) throw new Error('Enter a finite position offset.');
  const values: string[] = []; let start = 0; let depth = 0;
  const source = translate === 'none' ? '0px 0px' : translate;
  for (let index = 0; index <= source.length; index++) {
    if (source[index] === '(') depth++;
    if (source[index] === ')') depth--;
    if (index === source.length || /\s/.test(source[index]) && depth === 0) { if (index > start) values.push(source.slice(start, index)); start = index + 1; }
  }
  const style = document.createElement('div').style; style.cssText = inlineStyle;
  style.setProperty('translate', `calc(${values[0] ?? '0px'} + ${x}px) calc(${values[1] ?? '0px'} + ${y}px)${values[2] ? ` ${values[2]}` : ''}`, 'important');
  if (!style.getPropertyValue('translate')) throw new Error('This item cannot use that position.');
  return style.cssText;
}
export const escapeHtml = (value: string) => value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
export function imageFile(file: File): Promise<string> {
  if (file.size > 512 * 1024 || !['image/png', 'image/jpeg', 'image/webp', 'image/gif', 'image/svg+xml'].includes(file.type)) return Promise.reject(new Error('Choose a PNG, JPEG, WebP, GIF or SVG image up to 512 KB.'));
  return new Promise((resolve, reject) => { const reader = new FileReader(); reader.onerror = () => reject(new Error('The image could not be read.')); reader.onload = () => {
    const value = String(reader.result); const image = new Image(); image.onerror = () => reject(new Error('This file could not be opened as an image.'));
    image.onload = () => image.naturalWidth > 0 && image.naturalWidth <= 16384 && image.naturalHeight <= 16384 ? resolve(value) : reject(new Error('Choose an image no larger than 16,384 pixels on either side.'));
    image.src = value;
  }; reader.readAsDataURL(file); });
}
