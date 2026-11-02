// A runtime adapter: the shim's job for a Node or Bun program, done in JavaScript, with the program's
// binary left as it is (docs/USAGE.md#a-runtime-adapter-instead-of-the-shim). The runtime loads it
// before the program:
//
//   NODE_OPTIONS="--require /path/to/tty-adapter.cjs" node app.js
//   BUN_OPTIONS="--preload /path/to/tty-adapter.cjs" ./bun-built-program
//
// It makes the zeropty session's socket a terminal to the program's tty module and
// process.stdout/stderr/stdin, using only what every session has: ZEROPTY_TTY_ID (which file is the
// terminal), ZEROPTY_SIZE_FILE and SIGWINCH (the size), and the zeropty-tty OSC (the mode).
// Run by examples/node/adapter.sh.
(() => {
  const id = /^(-?\d+):(\d+)$/.exec(process.env.ZEROPTY_TTY_ID || '');
  // The programs it starts do not load the adapter again.
  delete process.env.NODE_OPTIONS;
  delete process.env.BUN_OPTIONS;
  if (!id) return;
  const fs = require('fs'), tty = require('tty');
  // st_dev is negative on macOS (-1:...) and ino can pass 2^53: compare as bigints.
  const ours = (fd) => {
    try {
      const s = fs.fstatSync(fd, { bigint: true });
      return BigInt.asIntN(32, s.dev) === BigInt.asIntN(32, BigInt(id[1])) && s.ino === BigInt(id[2]);
    } catch { return false }
  };
  const size = () => {
    let cols = Number(process.env.ZEROPTY_COLS) || 80, rows = Number(process.env.ZEROPTY_ROWS) || 24;
    try { [cols, rows] = fs.readFileSync(process.env.ZEROPTY_SIZE_FILE, 'utf8').trim().split(/\s+/).map(Number) } catch {}
    return [cols, rows];
  };
  const isatty = tty.isatty;
  tty.isatty = (fd) => ours(fd) || isatty(fd);
  const out = [process.stdout, process.stderr].filter((s) => ours(s.fd));
  for (const s of out) {
    s.isTTY = true;
    Object.defineProperty(s, 'columns', { get: () => size()[0], configurable: true });
    Object.defineProperty(s, 'rows', { get: () => size()[1], configurable: true });
    s.getWindowSize = size;
    s.hasColors = () => true;
    s.getColorDepth = () => 8;
  }
  process.on('SIGWINCH', () => out.forEach((s) => s.emit('resize')));
  if (out.length && ours(0)) {
    process.stdin.isTTY = true;
    // Raw (0) or a new terminal's modes (15), told to zeropty in the output stream.
    process.stdin.setRawMode = function (on) {
      out[0].write(on ? '\x1b]zeropty-tty;0\x07' : '\x1b]zeropty-tty;15\x07');
      this.isRaw = !!on;
      return this;
    };
  }
})();
