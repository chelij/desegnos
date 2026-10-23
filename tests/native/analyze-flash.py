"""Compare lossless owned-window captures, retaining every anomalous frame index.
Run: python tests/native/analyze-flash.py test-results/flash/before test-results/flash/after
The two unchanged content regions identify missing or unstyled canvas content;
DOM traces separately prove that a visible document survives each swap.
"""
import json
from pathlib import Path
import subprocess
import sys

import numpy as np
from PIL import Image, ImageDraw


def analyze(folder, kind):
    for stale in folder.glob(f'{kind}-flash-*.png'):
        stale.unlink()
    video = folder / f'{kind}.mkv'
    probe = json.loads(subprocess.check_output(['ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_streams', '-show_frames', '-of', 'json', str(video)]))
    stream = probe['streams'][0]
    width, height = stream['width'], stream['height']
    data = subprocess.check_output(['ffmpeg', '-loglevel', 'error', '-i', str(video), '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-'])
    frames = np.frombuffer(data, dtype=np.uint8).reshape(-1, height, width, 3)
    # These fixed fixture regions exclude the moved element, handles, pointer and notices.
    x1,y1,x2,y2 = (440,350,610,710) if kind == 'owner' else (360,337,605,356)
    roi = frames[:, y1:y2, x1:x2].astype(np.int16)
    delta = np.abs(roi-roi[0]).mean(axis=(1,2,3))
    app = frames[:, :80, :145].astype(np.int16)
    app_delta = np.abs(app-app[0]).mean(axis=(1,2,3))
    bad = np.flatnonzero(delta > 2).tolist()
    # Include all flash boundaries plus ordinary pre/post views.
    indices = sorted(set([0, len(frames)-1] + ([max(0,bad[0]-1),bad[0],bad[-1],min(len(frames)-1,bad[-1]+1)] if bad else [len(frames)//3,len(frames)//2,2*len(frames)//3])))
    strip = Image.new('RGB', (690*len(indices), 475), '#ffffff')
    draw = ImageDraw.Draw(strip)
    for column,index in enumerate(indices):
        im = Image.fromarray(frames[index]); im.thumbnail((690,450)); strip.paste(im,(690*column,25))
        draw.text((690*column+8,6), f'{kind} frame {index} / {probe["frames"][index]["best_effort_timestamp_time"]}s  ROI delta {delta[index]:.2f}',fill='black')
        if index in bad:
            Image.fromarray(frames[index]).save(folder/f'{kind}-flash-{index:04d}.png')
    strip.save(folder/f'{kind}-strip.png')
    trace=json.loads((folder/f'{kind}-trace.json').read_text())
    blank=[s['t'] for s in trace['frames'] if any(not any(f['visibility']!='hidden' and f.get('body',0)>0 for f in p['iframes']) for p in s['frames'])]
    return dict(frames=len(frames),fps=stream['r_frame_rate'],flashFrames=bad,flashTimes=[probe['frames'][n]['best_effort_timestamp_time'] for n in bad],maxCanvasDelta=float(delta.max()),maxAppDelta=float(app_delta.max()),emptyDocumentSamples=blank,alignment=trace['alignment'])


for name in sys.argv[1:]:
    folder=Path(name)
    results={kind:analyze(folder,kind) for kind in ['owner','simple']}
    (folder/'analysis.json').write_text(json.dumps(results,indent=2)+'\n')
    print(name,json.dumps(results))
