#!/usr/bin/env node
// A zeropty plugin in Node (client/plugin.mjs): it keeps a log of sessions coming and going, and of the
// moment each one last printed, and adds two commands:
//   zeropty notify-log            the last 100 entries
//   zeropty notify-idle [secs]    sessions quiet for that long (default 10), from what they printed
// With --desktop it also shows a macOS notification when a session ends.
//
//   node examples/plugins/notify.mjs [--desktop] &    # ZEROPTY_SOCK picks the server, as for the CLI
import { execFile } from 'node:child_process'
import { attachPlugin } from '../../client/plugin.mjs'

const desktop = process.argv.includes('--desktop')
const log = []
const lastOutput = new Map()
const note = (line) => {
  log.push(`${new Date().toISOString()} ${line}`)
  if (log.length > 100) log.shift()
}
// The text goes to AppleScript as an argument, never into its source: a session name may hold `"`
// (`new-session -s 'a"b'` is accepted), and pasted into the script it would end the string and run
// whatever follows (`do shell script ...`). The script itself is constant; `--` keeps a text that
// starts with `-` from being read as an option of osascript.
const notify = (text) =>
  execFile('osascript', ['-e', 'on run argv', '-e', 'display notification (item 1 of argv) with title "zeropty"', '-e', 'end run', '--', text], () => {})

const plugin = await attachPlugin({
  name: 'notify',
  events: ['session-created', 'session-exited', 'session-renamed', 'activity'],
  onEvent(ev) {
    switch (ev.event) {
      case 'activity': lastOutput.set(ev.session, Date.now()); break
      case 'session-created': note(`created ${ev.session}`); lastOutput.set(ev.session, Date.now()); break
      case 'session-renamed': note(`renamed ${ev.from} -> ${ev.session}`); lastOutput.set(ev.session, lastOutput.get(ev.from)); lastOutput.delete(ev.from); break
      case 'session-exited':
        note(`${ev.reason} ${ev.session}`)
        lastOutput.delete(ev.session)
        if (desktop) notify(`${ev.session} ${ev.reason}`)
        break
    }
  },
  commands: {
    'notify-log': () => log.map((l) => l + '\n').join(''),
    'notify-idle': async (argv, { plugin }) => {
      const secs = Number(argv[1] ?? 10)
      const { out } = await plugin.command(['list-sessions', '-F', '#{session_name}'])
      const quiet = out.split('\n').filter((s) => s && Date.now() - (lastOutput.get(s) ?? 0) >= secs * 1000)
      return quiet.map((s) => s + '\n').join('')
    },
  },
})
console.log('notify: attached (zeropty notify-log, zeropty notify-idle)')
await plugin.closed
