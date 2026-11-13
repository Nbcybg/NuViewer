"""Pull every embedded ICC profile (and the output intent) out of a PDF: python -I tools/extract-pdf-icc.py chart.pdf out_dir
(how crates/nv-codecs/profiles/CoatedFOGRA39.icc was taken from the user's "free CMYK chart" PDF)."""
import re, sys, zlib, hashlib, os
src, outdir = sys.argv[1], sys.argv[2]
os.makedirs(outdir, exist_ok=True)
data = open(src, 'rb').read()
print('size', len(data), 'header', data[:20])
# find every "N 0 obj ... stream ... endstream"
objs = list(re.finditer(rb'(\d+) 0 obj(.*?)endobj', data, re.S))
print('objects', len(objs))
for m in objs:
    body = m.group(2)
    num = int(m.group(1))
    head = body.split(b'stream', 1)[0]
    if b'ICCBased' in body or b'/N 4' in head or b'/N 3' in head or b'DestOutputProfile' in body or b'OutputIntent' in body or b'/Alternate' in head:
        print('obj', num, head[:300].replace(b'\n', b' '))
    if b'stream' in body and (b'/N 4' in head or b'/N 3' in head or b'/N 1' in head) and b'/Alternate' in head or b'/N 4' in head:
        s = body.split(b'stream', 1)[1]
        s = s.lstrip(b'\r\n')
        s = s.rsplit(b'endstream', 1)[0]
        if b'FlateDecode' in head:
            try:
                s = zlib.decompressobj().decompress(s)
            except Exception as e:
                print('inflate failed', e)
                continue
        if s[36:40] == b'acsp':
            cs = s[16:20]
            cls = s[12:16]
            print('  ICC profile obj', num, 'space', cs, 'class', cls, 'bytes', len(s), hashlib.md5(s).hexdigest())
            open(os.path.join(outdir, f'obj{num}_{cs.decode().strip()}.icc'), 'wb').write(s)
# also list image XObjects / content summary
for m in objs:
    head = m.group(2).split(b'stream', 1)[0]
    if b'/Subtype /Image' in head or b'/Subtype/Image' in head:
        print('image', m.group(1), head[:250].replace(b'\n', b' '))
