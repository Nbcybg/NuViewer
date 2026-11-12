import struct, sys, os

for path in sys.argv[1:]:
    size = os.path.getsize(path)
    with open(path, 'rb') as f:
        h = f.read(160)
    magic = h[:16]
    ver = h[16:20]
    model = h[28:60].split(b'\0')[0].decode('latin1')
    jpg_off, jpg_len, meta_off, meta_len, cfa_off, cfa_len = struct.unpack('>6I', h[84:108])
    print(f'{os.path.basename(path)}: file {size/1e6:.1f} MB, magic {magic!r}, model {model!r}')
    print(f'  embedded JPEG: offset {jpg_off}, length {jpg_len/1e6:.2f} MB ({100*jpg_len/size:.1f}% of file)')
    print(f'  meta: offset {meta_off}, len {meta_len}; CFA/raw: offset {cfa_off}, len {cfa_len/1e6:.1f} MB')
