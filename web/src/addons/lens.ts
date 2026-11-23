// Add-on for `pi-lens`: it adds its findings (diagnostics, test results, the same blockers again at every prompt) to
// the end of each request and takes them out of the next, so the prompt cache never finds the conversation it wrote
// and the whole of it is written again. The server can start pi with that turned off (`src/lens.rs`); the switch is
// shown only to a session that has pi-lens (its commands are there).

/** One of pi-lens's commands: the package is loaded in this session. */
export const COMMAND = 'lens-context-toggle';
