import { createReadStream } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const inputPath = join(__dirname, './input.md');
const stream = createReadStream(inputPath, { encoding: 'utf8', highWaterMark: 2 });

stream.on('data', (chunk) => {
  process.stdout.write(chunk);
  console.error(chunk);
  stream.pause();
  setTimeout(() => stream.resume(), 50);
});

stream.on('end', () => {
  // Done streaming
});

stream.on('error', (err) => {
  console.error('Error reading file:', err);
});
