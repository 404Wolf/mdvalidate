const data = "# Hi there\n";
let i = 0;

function writeNext() {
  if (i < data.length) {
    const chunk = data.slice(i, i + 2);
    process.stdout.write(chunk);
    console.error(chunk);
    i += 2;
    setTimeout(writeNext, 250);
  }
}

writeNext();
