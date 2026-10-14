/**
 * RISC Zero u32-word instruction serializer & SPEL JSON IDL decoder.
 */

export interface SpelIdlInstructionArg {
  name: string;
  type: string | Record<string, unknown>;
}

export interface SpelIdlInstruction {
  name: string;
  args: SpelIdlInstructionArg[];
}

export interface SpelProgramIdl {
  name: string;
  version: string;
  program_id_hex?: string;
  instructions: SpelIdlInstruction[];
}

/**
 * Encode a UTF-8 string into RISC Zero u32-word little-endian hex representation.
 */
export function encodeStringWordsHex(input: string): string {
  const bytes = new TextEncoder().encode(input);
  const paddedLen = Math.ceil((bytes.length + 4) / 4) * 4;
  const buf = new Uint8Array(paddedLen);
  const view = new DataView(buf.buffer);
  view.setUint32(0, bytes.length, true);
  buf.set(bytes, 4);
  return Array.from(buf)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}
