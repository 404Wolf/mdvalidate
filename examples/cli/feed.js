#!/usr/bin/env node

import { createReadStream } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const highWaterMark = parseInt(process.argv[2], 10) || 2;
const speed = parseInt(process.argv[3], 10) || 80;

const inputPath = join(__dirname, "./input.md");
const stream = createReadStream(inputPath, {
  encoding: "utf8",
  highWaterMark: highWaterMark,
});

stream.on("data", (chunk) => {
  process.stdout.write(chunk);
  console.error(chunk);
  stream.pause();
  setTimeout(() => stream.resume(), speed);
});

stream.on("end", () => {
  // Done streaming
});

stream.on("error", (err) => {
  console.error("Error reading file:", err);
});
