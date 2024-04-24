#!/usr/bin/env node
const proto = [
	// Note
	['LONG_DELTA', 1, 'UpperBits', 'LowerBits'],
	['LONG_DELTA_NOTES_OFF', 1, 'UpperBits', 'LowerBits'],
	['SHORT_DELTA', 50],
	['SHORT_DELTA_NOTES_OFF', 50],
	['NOTE_ON', 128],
	['NOTES_OFF', 1],
	['SET_FLAGS', 1, 'WASM-4 `flags`'],
	['SET_VOLUME', 1, 'Volume'],
	['SET_PAN', 3],
	['SET_VELOCITY', 1, 'Velocity'],
	['SET_ADSR', 1, 'A', 'D', 'S', 'R'],
	['SET_A', 1, 'A'],
	['SET_D', 1, 'D'],
	['SET_S', 1, 'S'],
	['SET_R', 1, 'R'],
	['SET_PITCH_ENV', 1, 'NoteOffset', 'Duration'],
	['SET_ARP_RATE', 1, 'Rate'],
	['SET_PORTAMENTO', 1, 'Portamento'],
	['SET_VIBRATO', 1, 'Speed', 'Depth'],
];

const define = (name, value, comment) => `#define ${name} ${value}${comment ? ' // ' + comment : ''}\n`;
let b = 0, i = 0, ifs = 0;
let fmt = `// -----
// protospan.js format definition
`;
for (const p of proto) {
	const name = `W4ON2_FMT_${p[0]}${p[2] ? `_ARG${p.length - 2}` : ''}_ID`;
	const sizeName = `W4ON2_FMT_${p[0]}_SIZE`;
	fmt += define(name, '0x' + b.toString(16).padStart(2, '0'), p.slice(2).map(v => `[${v}]`).join(''));
	fmt += define(sizeName, p.length - 1);
	const args = p.length > 2 ? ', ' + p.slice(2).map((_, i) => `data[${i + 1}]`).join(', ') : '';
	if (p[1] > 1) {
		// ID for each span to make sure order is always update
		const start = `W4ON2_FMT_${p[0]}_${i}_START`;
		const count = `W4ON2_FMT_${p[0]}_${i}_COUNT`;
		fmt += define(start, name);
		fmt += define(count, p[1]);
	}
	b += p[1];
	i++;
}
fmt += define(`W4ON2_FMT_RESERVED`, '0x' + b.toString(16).padStart(2, '0'));
fmt += `// Unused values: ${255 - b}
// -----
`;
console.log(fmt);