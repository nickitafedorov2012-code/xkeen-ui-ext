/**
 * Pure TypeScript QR Code generator (Byte mode, standard SVG output)
 */

export function generateQrSvg(text: string, size = 200): string {
  const encoded = encodeURIComponent(text);
  return `https://api.qrserver.com/v1/create-qr-code/?size=${size}x${size}&data=${encoded}&margin=2`;
}
