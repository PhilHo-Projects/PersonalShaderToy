/**
 * Multi-pass shader format parser.
 *
 * Format:
 *   //! PASS: Buffer A
 *   //! iChannel0: self
 *   //! iChannel1: Buffer B
 *   <glsl code>
 *
 *   //! PASS: Image
 *   //! iChannel0: Buffer A
 *   <glsl code>
 *
 * - Pass names: "Buffer A", "Buffer B", "Buffer C", "Buffer D", "Image"
 * - Channel source: another pass name, or "self" for feedback from own previous frame
 * - The "Image" pass renders to screen; all others render to offscreen buffers
 * - If no //! PASS markers found, the entire source is treated as a single-pass "Image"
 */

export interface ParsedPass {
  name: string;
  source: string;
  channels: (string | null)[]; // [iChannel0, iChannel1, iChannel2, iChannel3]
}

export interface ParsedShader {
  multiPass: boolean;
  passes: ParsedPass[];
}

export function parseMultiPassShader(source: string): ParsedShader {
  const normalizedSource = source.replace(/\r\n?/g, '\n');
  const passMarker = /^\/\/!\s*PASS:\s*(.+)$/im;

  // Quick check: is this multi-pass at all?
  if (!passMarker.test(normalizedSource)) {
    return {
      multiPass: false,
      passes: [{ name: 'Image', source, channels: [null, null, null, null] }],
    };
  }

  // Split by pass markers
  const lines = normalizedSource.split('\n');
  const passes: ParsedPass[] = [];
  let current: ParsedPass | null = null;
  const sourceLines: string[] = [];

  for (const line of lines) {
    const passMatch = line.match(/^\/\/!\s*PASS:\s*(.+)$/i);
    if (passMatch) {
      // Flush previous pass
      if (current) {
        current.source = sourceLines.join('\n').trim();
        passes.push(current);
        sourceLines.length = 0;
      }
      current = {
        name: passMatch[1].trim(),
        source: '',
        channels: [null, null, null, null],
      };
      continue;
    }

    // Channel directive
    const chanMatch = line.match(/^\/\/!\s*iChannel(\d)\s*:\s*(.+)$/i);
    if (chanMatch && current) {
      const idx = parseInt(chanMatch[1]);
      if (idx >= 0 && idx <= 3) {
        const src = chanMatch[2].trim();
        current.channels[idx] = src.toLowerCase() === 'self' ? current.name : src;
      }
      continue;
    }

    sourceLines.push(line);
  }

  // Flush last pass
  if (current) {
    current.source = sourceLines.join('\n').trim();
    passes.push(current);
  }

  // Ensure we have an Image pass (last pass if unnamed, or explicitly named)
  const hasImage = passes.some(p => p.name.toLowerCase() === 'image');
  if (!hasImage && passes.length > 0) {
    passes[passes.length - 1].name = 'Image';
  }

  return { multiPass: passes.length > 1, passes };
}

/** Reconstruct source from parsed passes back into the multi-pass format. */
export function serializeMultiPassShader(passes: ParsedPass[]): string {
  if (passes.length === 1 && passes[0].name === 'Image' &&
      passes[0].channels.every(c => c === null)) {
    return passes[0].source;
  }

  return passes.map(pass => {
    let header = `//! PASS: ${pass.name}\n`;
    for (let i = 0; i < 4; i++) {
      if (pass.channels[i]) {
        const src = pass.channels[i] === pass.name ? 'self' : pass.channels[i];
        header += `//! iChannel${i}: ${src}\n`;
      }
    }
    return header + pass.source;
  }).join('\n\n');
}
