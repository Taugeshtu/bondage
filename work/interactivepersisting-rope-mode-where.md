interactive/persisting rope mode, where we drop in without a prompt (whitespace prompt after expansion). and/or `-i` flag, for `interactive`.

because sometimes it do be taking more than one turn to solve the problem. Many more, sometimes!
So what do you do? You keep a file around that is the "session"; that file IS the messages rope (in a .rope folder in CWD?.. unless we don't have permissions, then in I dunno, ~/.rope/kebab-cased-dir-name?). That's step 1, we must be able to recover by closing and re-opening rope. To bootstrap back you can just rope -i @.rope/2026-0725.sesh or something like that :D

I don't think forking needs to be rope-native primitive. My rationale is: terminal UX suuuucks! We can do much better. So rope is my "emergency bailout". It's for small and targeted punches. Actual deep work will be happening in a bigger harness that WILL facilitate forking and all that jazz.
