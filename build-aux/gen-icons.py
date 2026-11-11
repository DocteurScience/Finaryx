#!/usr/bin/env python3
# Regenerates the navigation icons in data/icons/symbolic from the mockup
# stroke paths (design/*.dc.html). GTK recolours symbolic icons through
# their fill, so strokes are converted to filled outlines with shapely.
# Usage: python3 build-aux/gen-icons.py data/icons/symbolic
import re, sys
from shapely.geometry import LineString, Polygon
from shapely.ops import unary_union
ICONS = {
 'overview': 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z',
 'month': 'M5 5h14v15H5zM5 10h14M9 3v4M15 3v4',
 'cumulative': 'M5 20V10M12 20V4M19 20v-7',
 'forecast': 'M3 17l6-6 4 4 8-8M15 7h6v6',
 'accounts': 'M3 9l9-5 9 5M5 10v8M10 10v8M14 10v8M19 10v8M3 20h18',
 'import': 'M12 4v11M7 10l5 5 5-5M5 20h14',
}
def parse(d):
    toks = re.findall(r'[MLHVZmlhvz]|-?\d*\.?\d+', d)
    subs=[]; cur=None; x=y=0; i=0; cmd=None; closed=False
    while i < len(toks):
        t=toks[i]
        if re.match(r'[A-Za-z]', t):
            cmd=t; i+=1
            if cmd in 'Zz':
                cur.append(cur[0]); continue
            if cmd in 'Mm':
                pass
        if cmd in 'Mm':
            nx,ny=float(toks[i]),float(toks[i+1]); i+=2
            if cmd=='m': nx+=x; ny+=y
            x,y=nx,ny; cur=[(x,y)]; subs.append(cur); cmd='L' if cmd=='M' else 'l'
        elif cmd in 'Ll':
            nx,ny=float(toks[i]),float(toks[i+1]); i+=2
            if cmd=='l': nx+=x; ny+=y
            x,y=nx,ny; cur.append((x,y))
        elif cmd in 'Hh':
            v=float(toks[i]); i+=1; x = v if cmd=='H' else x+v; cur.append((x,y))
        elif cmd in 'Vv':
            v=float(toks[i]); i+=1; y = v if cmd=='V' else y+v; cur.append((x,y))
    return subs
def ring(coords):
    pts=list(coords)
    return 'M'+' L'.join(f'{px:.2f} {py:.2f}' for px,py in pts[:-1])+' Z'
for name,d in ICONS.items():
    geoms=[LineString(s).buffer(0.9, cap_style='round', join_style='round', quad_segs=6) for s in parse(d)]
    u=unary_union(geoms)
    polys = list(u.geoms) if hasattr(u,'geoms') else [u]
    parts=[]
    for p in polys:
        parts.append(ring(p.exterior.coords))
        parts += [ring(r.coords) for r in p.interiors]
    out=f'<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"><path fill="#222222" fill-rule="evenodd" d="{" ".join(parts)}"/></svg>\n'
    open(f'{sys.argv[1]}/finaryx-{name}-symbolic.svg','w').write(out)
    print(name, len(out))
