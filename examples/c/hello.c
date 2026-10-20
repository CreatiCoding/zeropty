/* The engine embedded in a C program through the C ABI (any language with an FFI works the same way).
 *
 *   ./build.sh
 *   cc examples/c/hello.c -Idist -Ldist -lzeropty_c -o /tmp/zeropty-hello
 *   DYLD_LIBRARY_PATH=dist /tmp/zeropty-hello
 *
 * Requests and replies are the socket protocol's JSON lines: {"argv": [...], "cwd": "..."} in,
 * {"code": 0, "out": "...", "err": "..."} out. */
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include "zeropty.h"

/* Runs one command and prints its reply. */
static void run(zeropty *zp, const char *request) {
  char *reply = zeropty_command(zp, request);
  printf("%s\n  -> %s\n", request, reply);
  zeropty_free(reply);
}

int main(void) {
  zeropty *zp = zeropty_open(NULL); /* or "{\"history\": 2000}" */
  if (!zp) return 1;
  run(zp, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"c\",\"-x\",\"40\",\"-y\",\"4\",\"exec cat\"],\"cwd\":\"/\"}");
  run(zp, "{\"argv\":[\"send-keys\",\"-t\",\"=c:\",\"-l\",\"hi from C\"],\"cwd\":\"/\"}");
  run(zp, "{\"argv\":[\"send-keys\",\"-t\",\"=c:\",\"Enter\"],\"cwd\":\"/\"}");
  usleep(100 * 1000); /* time for cat to print the line back */
  run(zp, "{\"argv\":[\"capture-pane\",\"-p\",\"-t\",\"=c:\"],\"cwd\":\"/\"}");
  run(zp, "{\"argv\":[\"kill-server\"],\"cwd\":\"/\"}");
  zeropty_close(zp);
  return 0;
}
