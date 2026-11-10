#!/usr/bin/env python3
"""Build the Julian-centered GIS candidate map and QGIS-compatible layers.

Input: reviewed public-directory selection in docs/testing/data/kiwisdr-candidates.json.
Requires geopandas, cartopy, matplotlib and pyproj. No receiver connections are
opened by this renderer. Natural Earth basemap data are downloaded on first use.
"""
import csv
import hashlib
import json
from pathlib import Path

import cartopy
import cartopy.crs as ccrs
import cartopy.feature as cfeature
import geopandas as gpd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.patheffects as pe
from matplotlib.patches import Circle
from matplotlib.path import Path as MplPath
import numpy as np
from pyproj import Geod
from shapely.geometry import Point, LineString

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / 'docs/testing/data'
FIGURES = ROOT / 'docs/testing/figures'
COLORS = {'control':'#667988','pilot':'#087f8c','expanded':'#b57a26','stretch':'#8c6bb1'}
GEOD = Geod(ellps='WGS84')


def main():
    doc=json.loads((DATA/'kiwisdr-candidates.json').read_text())
    tx=doc['transmitter'];nodes=doc['candidates'];lon0=tx['longitude'];lat0=tx['latitude']
    FIGURES.mkdir(parents=True,exist_ok=True)
    point_rows=[dict(candidate_id='TX',label=tx['label'],tier='transmitter',distance_km=0.,initial_bearing_deg=0.,url='',geometry=Point(lon0,lat0))]
    routes=[]
    for n in nodes:
        assert -90<=n['latitude']<=90 and -180<=n['longitude']<=180
        point_rows.append({k:n[k] for k in ['candidate_id','label','tier','distance_km','initial_bearing_deg','url']})
        point_rows[-1]['geometry']=Point(n['longitude'],n['latitude'])
        coords=[(lon0,lat0),*GEOD.npts(lon0,lat0,n['longitude'],n['latitude'],180),(n['longitude'],n['latitude'])]
        routes.append(dict(candidate_id=n['candidate_id'],distance_km=n['distance_km'],initial_bearing_deg=n['initial_bearing_deg'],tier=n['tier'],geometry=LineString(coords)))
    points=gpd.GeoDataFrame(point_rows,crs='EPSG:4326');lines=gpd.GeoDataFrame(routes,crs='EPSG:4326')
    # GeoPackage retains complete geodesics in a single GIS project data source.
    gpkg=DATA/'kiwisdr-candidates.gpkg'
    if gpkg.exists():gpkg.unlink()
    points.to_file(gpkg,layer='candidate_stations',driver='GPKG')
    lines.to_file(gpkg,layer='candidate_geodesics',driver='GPKG')
    points.to_file(DATA/'kiwisdr-candidates.geojson',driver='GeoJSON')
    with (DATA/'kiwisdr-candidates.csv').open('w',newline='') as f:
        keys=['candidate_id','registry_id','label','tier','latitude','longitude','distance_km','initial_bearing_deg','url','listed_status','listed_updated']
        writer=csv.DictWriter(f,fieldnames=keys);writer.writeheader()
        for n in nodes:writer.writerow({k:n[k] for k in keys})

    fig=plt.figure(figsize=(11,6.2),facecolor='#f7f9fa')
    fig.text(.035,.955,'LINKWISE  /  KIWISDR FIELD TEST',fontsize=9,fontweight='bold',color='#27626e')
    fig.text(.035,.906,'Candidate receive network from Julian, California',fontsize=18,fontweight='bold',color='#173744')
    fig.text(.035,.873,'WGS84 geodesics · approximate transmit locality · candidate sites, not confirmed links',fontsize=9,color='#5d717c')
    projection=ccrs.AzimuthalEquidistant(central_longitude=lon0,central_latitude=lat0)
    ax=fig.add_axes([.025,.12,.49,.72],projection=projection)
    radius=17_500_000
    theta=np.linspace(0,2*np.pi,512)
    ax.set_boundary(MplPath(np.column_stack([np.cos(theta),np.sin(theta)])*.5+.5),transform=ax.transAxes)
    ax.set_xlim(-radius,radius);ax.set_ylim(-radius,radius)
    ax.set_facecolor('#eaf1f5')
    ax.add_feature(cfeature.LAND.with_scale('110m'),facecolor='#d7e1dc',edgecolor='none',zorder=0)
    ax.add_feature(cfeature.COASTLINE.with_scale('110m'),edgecolor='#91a69f',linewidth=.4,zorder=1)
    ax.add_feature(cfeature.BORDERS.with_scale('110m'),edgecolor='#b2beb7',linewidth=.3,zorder=1)
    ax.gridlines(crs=ccrs.PlateCarree(),linewidth=.35,color='#a9bac4',alpha=.55,linestyle=':')
    for km in [4000,8000,12000,16000]:
        bearing=np.linspace(0,360,361);xs,ys,_=GEOD.fwd(np.full(361,lon0),np.full(361,lat0),bearing,np.full(361,km*1000.))
        ax.plot(xs,ys,transform=ccrs.Geodetic(),color='#98adb6',lw=.55,ls=(0,(3,4)),zorder=2)
        labx,laby,_=GEOD.fwd(lon0,lat0,215,km*1000.)
        ax.text(labx,laby,f'{km//1000},000 km',transform=ccrs.PlateCarree(),fontsize=7,color='#536d79',ha='center',path_effects=[pe.withStroke(linewidth=2.5,foreground='#f7f9fa')],zorder=6)
    # Great-circle lines are illustrative geometries, not propagation predictions.
    for route,n in zip(routes,nodes):
        x,y=route['geometry'].xy
        ax.plot(x,y,transform=ccrs.Geodetic(),lw=1.15 if n['tier']=='pilot' else .75,alpha=.8 if n['tier']=='pilot' else .55,color=COLORS[n['tier']],zorder=3)
        ax.plot(n['longitude'],n['latitude'],'o',ms=5.5,color=COLORS[n['tier']],mec='white',mew=.6,transform=ccrs.PlateCarree(),zorder=5)
        if n['candidate_id'] not in ['R01','R02','R03','R04']:
            xy=projection.transform_point(n['longitude'],n['latitude'],ccrs.PlateCarree())
            offsets={'R05':(-18,-14),'R06':(9,-10),'R07':(-27,7),'R08':(-27,-12),'R09':(8,-10),'R10':(9,5),'R11':(9,7)}
            ax.annotate(n['candidate_id'],xy=xy,xytext=offsets.get(n['candidate_id'],(7,7)),textcoords='offset points',url=n['url'],fontsize=9,fontweight='bold',color=COLORS[n['tier']],path_effects=[pe.withStroke(linewidth=3,foreground='white')],zorder=6)
    ax.plot(lon0,lat0,'*',ms=13,color='#cf5047',mec='white',mew=.8,transform=ccrs.PlateCarree(),zorder=8)
    ax.text(.5,-.045,'Azimuthal equidistant centered on Julian · rings show distance from TX',transform=ax.transAxes,ha='center',fontsize=8,color='#607884')

    # Regional inset expands the nearby markers without inventing exact TX placement.
    inset=fig.add_axes([.52,.57,.245,.245],projection=ccrs.AlbersEqualArea(central_longitude=-112,central_latitude=39,standard_parallels=(30,50)))
    inset.set_extent([-126,-105,30,50],ccrs.PlateCarree());inset.set_facecolor('#eaf1f5')
    inset.add_feature(cfeature.LAND.with_scale('50m'),facecolor='#dce6df',edgecolor='none')
    inset.add_feature(cfeature.COASTLINE.with_scale('50m'),edgecolor='#91a69f',lw=.5)
    inset.add_feature(cfeature.STATES.with_scale('50m'),edgecolor='#aabbb4',lw=.4)
    inset.set_title('Western US detail',loc='left',fontsize=11,fontweight='bold',color='#173744',pad=7)
    for n in nodes[:4]:
        inset.plot(n['longitude'],n['latitude'],'o',ms=5,color=COLORS[n['tier']],mec='white',mew=.5,transform=ccrs.PlateCarree(),zorder=5)
        x,y=inset.projection.transform_point(n['longitude'],n['latitude'],ccrs.PlateCarree())
        offsets={'R01':(8,-13),'R02':(8,-5),'R03':(8,0),'R04':(8,0)}
        inset.annotate(n['candidate_id'],(x,y),xytext=offsets[n['candidate_id']],textcoords='offset points',url=n['url'],fontsize=9,fontweight='bold',color=COLORS[n['tier']],zorder=6)
    inset.plot(lon0,lat0,'*',ms=10,color='#cf5047',transform=ccrs.PlateCarree(),zorder=7)
    txxy=inset.projection.transform_point(lon0,lat0,ccrs.PlateCarree())
    inset.annotate('Julian TX*',txxy,xytext=(3,10),textcoords='offset points',ha='left',fontsize=8,fontweight='bold',color='#b7443c',zorder=8)
    note=fig.add_axes([.785,.575,.185,.235]);note.axis('off')
    note.text(0,1,'A staged receiver set',fontsize=12,fontweight='bold',color='#173744',va='top')
    note.text(0,.79,'R01    nearby control\nR02 / R03 / R05    pilot pool\nR04 + R06–R10    expansion\nR11    stretch candidate',fontsize=9,color='#435e6b',linespacing=1.4,va='top')
    note.text(0,.30,'Use at most three concurrent\nreceivers in the first pilot.\nOwner access and RF readiness\nremain unconfirmed.',fontsize=8,color='#607884',linespacing=1.3,va='top')

    legend=fig.add_axes([.52,.16,.45,.36]);legend.axis('off')
    legend.text(0,1.04,'CANDIDATE REGISTER · PDF LINKS',fontsize=10,fontweight='bold',color='#27626e')
    legend.text(.77,1.04,'km',ha='right',fontsize=9,color='#607884');legend.text(.99,1.04,'bearing °T',ha='right',fontsize=9,color='#607884')
    for i,n in enumerate(nodes):
        y=.94-i*.081
        legend.text(0,y,n['candidate_id'],url=n['url'],fontsize=10.5,fontweight='bold',color=COLORS[n['tier']])
        legend.text(.1,y,n['label'],url=n['url'],fontsize=10.5,color='#173744')
        legend.text(.77,y,f"{n['distance_km']:,.0f}",ha='right',fontsize=7.5,color='#435e6b')
        legend.text(.99,y,f"{n['initial_bearing_deg']:.0f}°",ha='right',fontsize=7.5,color='#435e6b')
    fig.text(.035,.065,'* Julian locality reference: 33.0787° N, 116.6020° W. Replace with actual antenna coordinates before execution.',fontsize=7.5,color='#435e6b')
    fig.text(.035,.043,'Directory: KiwiSDR.com via rx.linkfanel.net mirror · source timestamp 09 Nov 2026 23:28 UTC · all candidates listed active in that snapshot.',fontsize=8,color='#607884')
    fig.text(.035,.023,'Geodesic lines do not predict usable HF paths. Basemap: Natural Earth. GIS: GeoPandas + Cartopy / PROJ. Exact receiver coordinates are owner-reported.',fontsize=8,color='#607884')
    fig.canvas.draw()
    renderer=fig.canvas.get_renderer()
    for text in fig.texts:
        box=text.get_window_extent(renderer)
        assert box.x0>=0 and box.x1<=fig.bbox.width and box.y0>=0 and box.y1<=fig.bbox.height, text.get_text()
    stem=FIGURES/'kiwisdr-candidate-map'
    fig.savefig(stem.with_suffix('.pdf'),metadata={'Title':'Linkwise KiwiSDR candidates from Julian, CA','Author':'Linkwise project','CreationDate':None,'ModDate':None})
    fig.savefig(stem.with_suffix('.png'),dpi=180)
    plt.close(fig)
    manifest={'map_source':'tools/build-kiwisdr-candidate-map.py','selection_source':'docs/testing/data/kiwisdr-candidates.json','projection':projection.proj4_init,'datum':'WGS84','geodesic_method':'pyproj.Geod WGS84 ellipsoid; shortest geodesic; initial forward bearing clockwise from true north','basemap':'Natural Earth via Cartopy; 110m global and 50m western-US detail','basemap_source':'https://www.naturalearthdata.com/','software':{'geopandas':gpd.__version__,'cartopy':cartopy.__version__,'matplotlib':matplotlib.__version__},'selection_sha256':hashlib.sha256((DATA/'kiwisdr-candidates.json').read_bytes()).hexdigest(),'artifacts':{str(path.relative_to(ROOT)):hashlib.sha256(path.read_bytes()).hexdigest() for path in [stem.with_suffix('.pdf'),stem.with_suffix('.png'),gpkg,DATA/'kiwisdr-candidates.geojson',DATA/'kiwisdr-candidates.csv']}}
    (DATA/'kiwisdr-map-provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(stem.with_suffix('.pdf'))


if __name__=='__main__':
    main()
