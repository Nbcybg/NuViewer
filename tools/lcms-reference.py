"""python -I tools/lcms-reference.py profile.icc out.json (needs Pillow).
Reference conversions with LittleCMS (through Pillow's ImageCms): FOGRA39 CMYK -> sRGB for a set of patches,
every rendering intent, with and without black point compensation. Independent of moxcms, used to judge it."""
import sys, json
from PIL import Image, ImageCms

prof_path = sys.argv[1]
src = ImageCms.getOpenProfile(prof_path)
dst = ImageCms.createProfile('sRGB')
print('profile:', ImageCms.getProfileDescription(src).strip())

patches = {
    'paper (0,0,0,0)': (0, 0, 0, 0),
    'C100': (100, 0, 0, 0),
    'M100': (0, 100, 0, 0),
    'Y100': (0, 0, 100, 0),
    'K100': (0, 0, 0, 100),
    'R (M100 Y100)': (0, 100, 100, 0),
    'G (C100 Y100)': (100, 0, 100, 0),
    'B (C100 M100)': (100, 100, 0, 0),
    'CMY 100': (100, 100, 100, 0),
    'rich black (60 40 40 100)': (60, 40, 40, 100),
    '4 x 100': (100, 100, 100, 100),
    'K50': (0, 0, 0, 50),
}
intents = {'perceptual': 0, 'relative': 1, 'saturation': 2, 'absolute': 3}
out = {}
for iname, i in intents.items():
    for bpc in (False, True):
        flags = ImageCms.Flags.BLACKPOINTCOMPENSATION if bpc else 0
        t = ImageCms.buildTransform(src, dst, 'CMYK', 'RGB', renderingIntent=i, flags=flags)
        row = {}
        for name, (c, m, y, k) in patches.items():
            px = Image.new('CMYK', (1, 1), tuple(round(v * 255 / 100) for v in (c, m, y, k)))
            rgb = ImageCms.applyTransform(px, t).getpixel((0, 0))
            row[name] = rgb
        out[f'{iname}{"+BPC" if bpc else ""}'] = row
json.dump(out, open(sys.argv[2], 'w'), indent=1)
names = list(patches)
print('%-26s' % 'patch', *['%-16s' % k for k in out])
for n in names:
    print('%-26s' % n, *['%-16s' % (out[k][n],) for k in out])
