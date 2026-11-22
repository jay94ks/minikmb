"""Conservative grid router for prototype connectivity, not an SI sign-off tool.

KiCad DRC and netlist parity are authoritative. High-speed skew/impedance must be
reviewed against a fabricator's stackup before fabrication release.
"""
from __future__ import annotations

import heapq
import itertools
import json
import math
import sys
from array import array
from collections import defaultdict
from pathlib import Path

import pcbnew as p

ROOT=Path(__file__).resolve().parents[1]/'boards'/'ms2130-test'
PATH=ROOT/'MS2130_Test.kicad_pcb'
OUTPUT=ROOT/'review'/'EXPERIMENTAL_AUTO_ROUTE_NOT_FOR_FAB.kicad_pcb'
GRID=.1
X0,Y0=25.91,20.0
NX,NY=631,551
W=.18
CLR=.127
HALO=CLR+W/2+.025


def mm(n): return p.ToMM(n)
def v(x,y): return p.VECTOR2I(p.FromMM(x),p.FromMM(y))
def coord(x,y): return round((x-X0)/GRID),round((y-Y0)/GRID)
def xy(a): return X0+a[0]*GRID,Y0+a[1]*GRID
def index(x,y): return y*NX+x


class Router:
    def __init__(self):
        self.b=p.LoadBoard(str(PATH))
        self.layers=[p.F_Cu,p.B_Cu]
        self.grid=[array('i',[0])*(NX*NY) for _ in range(2)]
        self.pads=defaultdict(list)
        self.names={}
        self.failed=[]
        self.paths=[]
        self.vias=[]
        self.escape={}
        for fp in sorted(self.b.GetFootprints(),key=lambda f:f.GetReference()):
            for pad in fp.Pads():
                n=pad.GetNetCode()
                self.names[n]=pad.GetNetname().lstrip('/')
                self.pads[n].append(pad)
                bb=pad.GetBoundingBox()
                # NPTH pads have a physical hole, represented by their courtyard-sized pad.
                for li,layer in enumerate(self.layers):
                    if pad.IsOnLayer(layer) or pad.GetAttribute()==p.PAD_ATTRIB_NPTH:
                        self.rect(li,mm(bb.GetX())-HALO,mm(bb.GetY())-HALO,mm(bb.GetRight())+HALO,mm(bb.GetBottom())+HALO,n or -3)

    def mark(self,li,x,y,n):
        if not (0<=x<NX and 0<=y<NY): return
        i=index(x,y); old=self.grid[li][i]
        self.grid[li][i]=n if old in (0,n) else -1

    def rect(self,li,x1,y1,x2,y2,n):
        a,b=coord(x1,y1),coord(x2,y2)
        for y in range(max(0,math.ceil((y1-Y0)/GRID)),min(NY-1,math.floor((y2-Y0)/GRID))+1):
            for x in range(max(0,math.ceil((x1-X0)/GRID)),min(NX-1,math.floor((x2-X0)/GRID))+1):
                self.mark(li,x,y,n)

    def circle(self,li,cx,cy,r,n):
        a=coord(cx-r,cy-r); b=coord(cx+r,cy+r)
        for y in range(max(0,a[1]),min(NY-1,b[1])+1):
            for x in range(max(0,a[0]),min(NX-1,b[0])+1):
                xx,yy=xy((x,y))
                if (xx-cx)**2+(yy-cy)**2<=r*r:
                    self.mark(li,x,y,n)

    def free(self,x,y,l,n):
        return 4<=x<NX-4 and 4<=y<NY-4 and self.grid[l][index(x,y)] in (0,n)

    def viafree(self,x,y,n):
        # Existing pad/track halo already contains trace half-width + clearance.
        # Add via radius minus trace half-width, rounded up conservatively.
        xx,yy=xy((x,y))
        for vx,vy,vn in self.vias:
            dist=math.hypot(xx-vx,yy-vy)
            if dist<.01 and vn==n: return True
            if dist<.54: return False
        for l in (0,1):
            for dx in range(-3,4):
                for dy in range(-3,4):
                    if dx*dx+dy*dy>10: continue
                    if not self.free(x+dx,y+dy,l,n): return False
        return True

    def endpoint(self,pad):
        key=(pad.GetParentFootprint().GetReference(),pad.GetNumber())
        if key in self.escape:
            a=coord(*self.escape[key]); return [(a[0],a[1],0),(a[0],a[1],1)]
        a=coord(mm(pad.GetPosition().x),mm(pad.GetPosition().y))
        ls=[i for i,l in enumerate(self.layers) if pad.IsOnLayer(l)]
        return [(a[0],a[1],l) for l in ls]

    def astar(self,starts,goals,n,allowvia=True,limit=65000):
        ends=set(goals)
        gx,gy,_=goals[0]
        def h(x,y,l):
            return min(abs(x-a)+abs(y-b)+abs(l-c)*20 for a,b,c in goals)
        todo=[]; seq=itertools.count(); cost={}; prev={}
        for s in starts:
            if not self.free(*s,n): continue
            cost[s]=0; heapq.heappush(todo,(h(*s)*1.05,next(seq),s))
        visited=0
        while todo:
            _,_,a=heapq.heappop(todo)
            if a in ends:
                result=[a]
                while a in prev: a=prev[a]; result.append(a)
                return result[::-1]
            visited+=1
            if visited>limit: return None
            x,y,l=a; base=cost[a]
            for dx,dy,inc in ((1,0,1),(-1,0,1),(0,1,1),(0,-1,1),(1,1,1.414),(1,-1,1.414),(-1,1,1.414),(-1,-1,1.414)):
                xx,yy=x+dx,y+dy
                if not self.free(xx,yy,l,n): continue
                if dx and dy and not (self.free(x+dx,y,l,n) and self.free(x,y+dy,l,n)): continue
                c=(xx,yy,l); new=base+inc
                if new>=cost.get(c,1e20): continue
                cost[c]=new; prev[c]=a; heapq.heappush(todo,(new+h(*c)*1.05,next(seq),c))
            if allowvia and self.viafree(x,y,n):
                c=(x,y,1-l); new=base+45
                if new<cost.get(c,1e20):
                    cost[c]=new; prev[c]=a; heapq.heappush(todo,(new+h(*c)*1.05,next(seq),c))
        return None

    def track(self,a,b,layer,n,width=W):
        if math.dist(a,b)<.00001: return
        t=p.PCB_TRACK(self.b); t.SetStart(v(*a)); t.SetEnd(v(*b)); t.SetLayer(self.layers[layer]); t.SetWidth(p.FromMM(width)); t.SetNetCode(n); self.b.Add(t)
        dist=math.dist(a,b); steps=max(1,math.ceil(dist/(GRID/2)))
        for i in range(steps+1):
            f=i/steps
            self.circle(layer,a[0]+(b[0]-a[0])*f,a[1]+(b[1]-a[1])*f,width/2+W/2+CLR+.055,n)

    def via(self,x,y,n):
        if any(math.hypot(x-a,y-b)<.01 and n==c for a,b,c in self.vias): return
        c=p.PCB_VIA(self.b); c.SetPosition(v(x,y)); c.SetWidth(p.FromMM(.55)); c.SetDrill(p.FromMM(.3)); c.SetViaType(p.VIATYPE_THROUGH); c.SetLayerPair(p.F_Cu,p.B_Cu); c.SetNetCode(n); self.b.Add(c)
        for layer in (0,1): self.circle(layer,x,y,.275+HALO,n)
        self.vias.append((x,y,n))

    def emit(self,path,n,startpad=None,endpad=None):
        if startpad:
            key=(startpad.GetParentFootprint().GetReference(),startpad.GetNumber())
            pos=startpad.GetPosition(); a=self.escape.get(key,(mm(pos.x),mm(pos.y)))
            self.track(a,xy(path[0]),path[0][2],n)
        run=path[0]
        for i in range(1,len(path)):
            a,b=path[i-1],path[i]
            if a[2]!=b[2]:
                self.track(xy(run),xy(a),a[2],n); self.via(*xy(a),n); run=b
            elif i==len(path)-1 or path[i+1][2]!=b[2] or (b[0]-a[0],b[1]-a[1])!=(path[i+1][0]-b[0],path[i+1][1]-b[1]):
                self.track(xy(run),xy(b),b[2],n); run=b
        if endpad:
            key=(endpad.GetParentFootprint().GetReference(),endpad.GetNumber())
            pos=endpad.GetPosition(); a=self.escape.get(key,(mm(pos.x),mm(pos.y)))
            self.track(xy(path[-1]),a,path[-1][2],n)
        self.paths.append((n,path))

    def ground(self):
        n=next(k for k,v in self.names.items() if v=='GND')
        # Nine EP thermal vias, kept out of stencil apertures during assembly review.
        for dx in (-1.6,0,1.6):
            for dy in (-1.6,0,1.6): self.via(52+dx,46+dy,n)
        pads=[pad for pad in self.pads[n] if pad.GetAttribute()==p.PAD_ATTRIB_SMD]
        for pad in pads:
            if (pad.GetParentFootprint().GetReference(),pad.GetNumber()) in self.escape: continue
            if pad.GetParentFootprint().GetReference()=='U1' and pad.GetNumber()=='65': continue
            starts=self.endpoint(pad); sx,sy,l=starts[0]
            # Smallest safe ground escape to an all-layer via; do not blindly drill pads.
            possible=[]
            for rad in range(6,26,2):
                for dx,dy in ((rad,0),(-rad,0),(0,rad),(0,-rad),(rad,rad),(-rad,rad),(rad,-rad),(-rad,-rad)):
                    x,y=sx+dx,sy+dy
                    if self.viafree(x,y,n): possible.append((x,y,l))
                if possible: break
            path=self.astar(starts,possible,n,False,10000) if possible else None
            if path:
                self.emit(path,n,startpad=pad); self.via(*xy(path[-1]),n)
            else:
                self.failed.append('GND '+pad.GetParentFootprint().GetReference()+'.'+pad.GetNumber())

    def fanout(self):
        # Reserve escape channels before routing can trap 0.5mm-pitch pads.
        for n,pads in self.pads.items():
            if n<=0: continue
            for pad in pads:
                ref,num=pad.GetParentFootprint().GetReference(),pad.GetNumber()
                if ref not in ('U1','J1') or not num.isdigit(): continue
                num=int(num); a=(mm(pad.GetPosition().x),mm(pad.GetPosition().y))
                if ref=='U1' and num<=64:
                    side,i=divmod(num-1,16); spread=i-7.5
                    if side==0: b=(52+spread,55)
                    elif side==1: b=(61,46-spread)
                    elif side==2: b=(52-spread,37)
                    else: b=(45,46+spread)
                elif ref=='J1' and num<=19:
                    b=(40.5,56-num)
                else: continue
                self.track(a,b,0,n,.15); self.via(*b,n)
                self.escape[(ref,str(num))]=b

    def net(self,n,onlyfront=False):
        pads=self.pads[n]
        if len(pads)<2: return
        # Route a minimum spanning tree of physical pads, preserving connector TVS nodes.
        connected=[pads[0]]; pending=pads[1:]
        while pending:
            a,b=min(((a,b) for a in connected for b in pending),key=lambda ab:math.dist((mm(ab[0].GetPosition().x),mm(ab[0].GetPosition().y)),(mm(ab[1].GetPosition().x),mm(ab[1].GetPosition().y))))
            starts,goals=self.endpoint(a),self.endpoint(b)
            if onlyfront: starts=[s for s in starts if s[2]==0]; goals=[s for s in goals if s[2]==0]
            path=self.astar(starts,goals,n,not onlyfront) if starts and goals else None
            if not path and onlyfront:
                path=self.astar(self.endpoint(a),self.endpoint(b),n,True)
            if path:
                self.emit(path,n,a,b)
            else:
                self.failed.append(self.names[n]+' '+a.GetParentFootprint().GetReference()+'.'+a.GetNumber()+' -> '+b.GetParentFootprint().GetReference()+'.'+b.GetNumber())
            pending.remove(b); connected.append(b)

    def run(self):
        done=set()
        self.fanout()
        # TMDS and SuperSpeed before low-speed traffic and power routing.
        for prefix in ('HDMI_D2','HDMI_D1','HDMI_D0','HDMI_C_','USB_CTX','USB_TX','USB_RX','USB_D_','XTIN','XTOUT','HDMI_REXT','USB_REXT'):
            for n,name in sorted(self.names.items()):
                if n>0 and name.startswith(prefix) and n not in done:
                    print('Route critical',name,flush=True); self.net(n,True); done.add(n)
        print('Ground escapes',flush=True); self.ground()
        remain=[n for n in self.pads if n>0 and self.names[n]!='GND' and n not in done]
        remain.sort(key=lambda n:(len(self.pads[n]),self.names[n]))
        for n in remain:
            print('Route',self.names[n],len(self.pads[n]),flush=True); self.net(n)
        p.SaveBoard(str(OUTPUT),self.b)
        report={'router':'prototype connectivity grid router; NOT impedance/skew signoff','failed_connections':self.failed,'routed_paths':len(self.paths),'vias':len(self.vias),'track_width_mm':W,'grid_mm':GRID}
        (ROOT/'review'/'routing-status.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
        print(json.dumps(report,indent=2),flush=True)


if __name__=='__main__':
    if '--experimental' not in sys.argv:
        raise SystemExit('Experimental, NOT SI/DRC-qualified. Explicit --experimental required. Output is review/EXPERIMENTAL_AUTO_ROUTE_NOT_FOR_FAB.kicad_pcb; default project is never overwritten.')
    Router().run()
