"""CRLF-tolerant exact-match patch helper. Tokens in old/new: <NL> = backslash+n (two chars), <T> = backslash+t, <BS> = backslash."""
import sys

BS = chr(92)


def tok(x):
    return x.replace('<NL>', BS + 'n').replace('<T>', BS + 't').replace('<BS>', BS)


def patch(path, pairs):
    s = open(path, encoding='utf-8', newline='').read()
    crlf = '\r\n' in s
    for old, new in pairs:
        old, new = tok(old), tok(new)
        if crlf:
            old = old.replace('\r\n', '\n').replace('\n', '\r\n')
            new = new.replace('\r\n', '\n').replace('\n', '\r\n')
        n = s.count(old)
        if n != 1:
            print('FAILED', path, 'matches:', n, '\n---\n' + old[:200])
            sys.exit(1)
        s = s.replace(old, new)
    open(path, 'w', encoding='utf-8', newline='').write(s)
    print('patched', path)
