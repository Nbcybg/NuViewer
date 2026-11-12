import struct, sys

path = sys.argv[1]
b = open(path, 'rb').read()
bo = '<' if b[:2] == b'II' else '>'
print('byte order', b[:2], 'magic', struct.unpack(bo + 'H', b[2:4])[0], 'size', len(b))

TYPES = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 6: 1, 7: 1, 8: 2, 9: 4, 10: 8, 11: 4, 12: 8, 13: 4, 16: 8}
NAMES = {
    254: 'NewSubfileType', 256: 'ImageWidth', 257: 'ImageLength', 258: 'BitsPerSample', 259: 'Compression',
    262: 'Photometric', 271: 'Make', 272: 'Model', 273: 'StripOffsets', 274: 'Orientation', 277: 'SamplesPerPixel',
    278: 'RowsPerStrip', 279: 'StripByteCounts', 322: 'TileWidth', 323: 'TileLength', 324: 'TileOffsets',
    325: 'TileByteCounts', 330: 'SubIFDs', 513: 'JPEGInterchangeFormat', 514: 'JPEGInterchangeFormatLength',
    33421: 'CFARepeatPatternDim', 33422: 'CFAPattern', 50706: 'DNGVersion', 50708: 'UniqueCameraModel',
    50829: 'ActiveArea', 50719: 'DefaultCropOrigin', 50720: 'DefaultCropSize', 34665: 'ExifIFD', 50740: 'DNGPrivateData',
    700: 'XMP', 34675: 'ICC', 50935: 'CompressionVariant?', 51041: 'NoiseProfile',
}
COMP = {1: 'none', 6: 'old-JPEG', 7: 'JPEG/lossless-JPEG', 8: 'deflate', 34892: 'lossy-DNG-JPEG', 52546: 'JPEG-XL', 32773: 'packbits'}


def read_ifd(off, depth, label):
    if off <= 0 or off + 2 > len(b):
        return
    n = struct.unpack(bo + 'H', b[off:off + 2])[0]
    print('  ' * depth + f'IFD {label} @ {off}: {n} entries')
    subs = []
    for i in range(n):
        e = b[off + 2 + i * 12: off + 14 + i * 12]
        tag, typ, cnt = struct.unpack(bo + 'HHI', e[:8])
        sz = TYPES.get(typ, 1) * cnt
        vo = e[8:12]
        data = vo if sz <= 4 else b[struct.unpack(bo + 'I', vo)[0]: struct.unpack(bo + 'I', vo)[0] + sz]
        fmt = {1: 'B', 3: 'H', 4: 'I', 8: 'h', 9: 'i', 6: 'b', 7: 'B'}.get(typ)
        val = None
        if typ == 2:
            val = data[:min(cnt, 40)].split(b'\0')[0].decode('latin1', 'replace')
        elif fmt:
            k = min(cnt, 6)
            val = list(struct.unpack(bo + fmt * k, data[:k * TYPES[typ]]))
        name = NAMES.get(tag, str(tag))
        extra = ''
        if tag == 259 and val:
            extra = ' (' + COMP.get(val[0], '?') + ')'
        if tag in (254, 256, 257, 258, 259, 262, 274, 277, 322, 323, 273, 279, 324, 325, 513, 514, 271, 272, 50708, 50706, 33422, 50719, 50720, 50829) or tag == 330:
            print('  ' * depth + f'  {name} = {val}{" ..." if cnt > 6 and typ != 2 else ""}{extra}  [count {cnt}]')
        if tag == 330:
            subs = list(struct.unpack(bo + 'I' * cnt, data[:4 * cnt])) if typ in (4, 13) else []
        if tag == 34665 and val:
            read_ifd(val[0], depth + 1, 'Exif')
    nxt = struct.unpack(bo + 'I', b[off + 2 + n * 12: off + 6 + n * 12])[0]
    for j, s in enumerate(subs):
        read_ifd(s, depth + 1, f'Sub{j}')
    if nxt:
        read_ifd(nxt, depth, 'next')


read_ifd(struct.unpack(bo + 'I', b[4:8])[0], 0, '0')

# every JPEG SOI with its frame size
print('JPEG streams found by byte scan:')
p = 0
while True:
    p = b.find(b'\xff\xd8\xff', p)
    if p < 0:
        break
    i = p + 2
    dims = None
    for _ in range(64):
        if i + 4 > len(b) or b[i] != 0xFF:
            break
        m = b[i + 1]
        if m == 0xFF:
            i += 1
            continue
        if m in (0xC0, 0xC1, 0xC2, 0xC3):
            h, w = struct.unpack('>HH', b[i + 5:i + 9])
            dims = (w, h, 'SOF%d' % (m - 0xC0))
            break
        if m == 0xDA:
            break
        i += 2 + struct.unpack('>H', b[i + 2:i + 4])[0]
    print('  offset', p, 'dims', dims)
    p += 3
