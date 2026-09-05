import init, {BrowserPlayer} from '../inspector/pkg/titan_game.js';
const result=document.getElementById('result');
const backend=new URL(location.href).searchParams.get('backend')??'webgpu';
const checks=[];
const captures={};
const evidence=document.createElement('section');document.body.append(evidence);
function publish(data){
 result.textContent=JSON.stringify(data,null,2);
 const bytes=JSON.stringify({...data,captures});
 if(bytes.length>40*1024*1024)throw Error('capture evidence exceeds 40 MiB');
 const link=document.createElement('a');link.textContent='Download capture evidence JSON';
 link.href=URL.createObjectURL(new Blob([bytes],{type:'application/json'}));
 link.download=`collection-room-${backend}-evidence.json`;evidence.prepend(link);
}
const check=(value,message)=>{if(!value)throw Error(message);checks.push(message);};
const canvas=document.querySelector('canvas');
let player, timedOut=false, sequence=0;
const deadline=setTimeout(()=>{timedOut=true;result.textContent=JSON.stringify({status:'failed',backend,error:'60-second browser deadline expired; reload to discard session',checks});},60000);
try {
 await init(); player=await BrowserPlayer.create(canvas,backend);
 const state=()=>JSON.parse(player.status());
 const request=async request=>{
   const request_id=`gpu-${++sequence}`;
   const response=JSON.parse(await player.dispatch(JSON.stringify({schema_version:2,request_id,request})));
   if(response.request_id!==request_id)throw Error('response/request correlation mismatch');
   return response;
 };
 check(player.frame(0),'initial GPU frame presented');
 check((await request({type:'step',frames:1})).error.code==='mutation_disabled','browser controls default disabled');
 const capture=async (name, retiring=false)=>{
   const before=await request({type:'status'});
   let outcome=await request({type:'capture'});
   const until=performance.now()+5000;
   while(retiring&&outcome.status==='failure'&&outcome.error.code==='busy'&&performance.now()<until){
     await new Promise(resolve=>setTimeout(resolve,4));
     outcome=await request({type:'capture'});
   }
   const after=await request({type:'status'});
   check(outcome.status==='success',`${name}: capture succeeds`);
   const value=outcome.response, identity=value.identity;
   check(['observed_frame','state_revision'].every(key=>before[key]===outcome[key]&&outcome[key]===identity[key]&&identity[key]===after[key]),`${name}: immutable frame/revision and no capture tick`);
   check(identity.width===960&&identity.height===540&&typeof identity.instance_id==='string'&&identity.capture_id>0&&value.width===960&&value.height===540&&value.format==='png'&&value.artifact.startsWith('data:image/png;base64,'),`${name}: owned 960x540 PNG`);
   captures[name]={...value,state:state()};
   const figure=document.createElement('figure'),caption=document.createElement('figcaption'),image=document.createElement('img');
   caption.textContent=`${name}: frame ${identity.observed_frame}, revision ${identity.state_revision}, generation ${identity.session_generation}`;
   image.src=value.artifact;image.width=960;image.height=540;figure.append(caption,image);evidence.append(figure);
   return value;
 };
 const initial=await capture('initial');
 check((await capture('initial-repeat')).checksum===initial.checksum,'read-only repeated capture pixels stable');
 player.set_control_enabled(true);

 player.resume();
 for(const [key,ticks] of [['ArrowRight',8],['ArrowUp',20],['ArrowRight',16]]) {
   player.set_key(key,true,false);
   for(let tick=0;tick<ticks;tick++) player.frame(1000/60+0.000001);
   player.set_key(key,false,false);
 }
 player.pause();
 const live=state();
 check(live.session_tick===44&&live.collected===3&&live.completed&&live.position.x===3000&&live.position.z===-2000,'44-tick keyboard route matches headless semantics');
 const win=await capture('win');
 check(win.checksum!==initial.checksum,'win image differs from initial');
 const recording=player.recording();
 player.load_recording(recording);player.resume();
 for(let tick=0;tick<44;tick++)player.frame(1000/60+0.000001);
 const replay=state();
 check(replay.session_tick===44&&replay.collected===3&&replay.completed&&replay.position.x===3000&&replay.position.z===-2000&&player.paused(),'interactive recording replay completes at exactly 44 ticks and pauses');
 check((await capture('replay')).checksum===win.checksum,'replay image equals same-backend 44-tick image');
 player.resize(0,0);check(!player.frame(0)&&state().surface.suspended,'zero-sized canvas suspends presentation');
 check((await capture('suspended')).checksum===win.checksum,'suspended surface permits identical offscreen capture');
 player.resize(640,360);check(player.frame(0)&&!state().surface.suspended,'resize restores GPU presentation');
 player.resize(960,540);player.frame(0);
 player.restart();player.resume();player.set_key('KeyD',true,false);player.set_key('KeyD',false,false);player.frame(1000/60+0.000001);player.pause();
 check(state().position.x===-2750,'tap released before a tick still moves once');
 player.restart();player.resume();player.set_key('KeyD',true,false);player.clear_input();player.frame(1000/60+0.000001);player.pause();
 check(state().position.x===-3000,'focus cancellation discards held input and taps');
 player.replay_route();player.step();check(state().session_tick===1&&player.paused(),'paused replay step consumes one tick');
 player.restart();check(state().session_tick===0&&state().collected===0&&!state().playback.active,'restart clears progress and playback');
 const reset=await capture('reset');
 check(reset.checksum===initial.checksum&&reset.identity.session_generation>initial.identity.session_generation,'reset restores image with newer session generation');
 for(const [name,x,z] of [['depth-behind',0,-1000],['depth-front',0,1500],['projection-far',-3000,-3000]]){
   const before=await request({type:'status'});
   const changed=await request({type:'invoke',name:'teleport',arguments:{x,z}});
   check(changed.status==='success'&&changed.observed_frame===before.observed_frame&&changed.state_revision>before.state_revision,`${name}: paused teleport changes revision without tick`);
   const fresh=await capture(name);
   check(fresh.identity.state_revision===changed.state_revision&&fresh.checksum!==initial.checksum,`${name}: fresh mutated scene without presentation`);
 }
 // Mutating during an outstanding capture must leave its owned scene/revision intact.
 const accepted=await request({type:'status'});
 const frozen=request({type:'capture'});
 const moved=request({type:'invoke',name:'teleport',arguments:{x:0,z:1500}});
 const pinned=await frozen;
 check(pinned.status==='success'&&pinned.response.checksum===captures['projection-far'].checksum&&pinned.state_revision===accepted.state_revision&&pinned.response.identity.state_revision===accepted.state_revision,'pending capture owns acceptance scene despite paused mutation');
 check((await moved).status==='success','mutation remains available while capture waits');
 check((await capture('after-pending-mutation')).checksum===captures['depth-front'].checksum,'next capture observes concurrent mutation');
 // Dispatch starts synchronously, but owns its data before its Promise yields.
 const pending=request({type:'capture'});
 const overlapping=request({type:'capture'});
 player.restart();
 const busy=await overlapping;
 check(busy.status==='failure'&&busy.error.code==='busy','overlapping capture is bounded busy');
 const cancelled=await pending;
 check(cancelled.status==='failure'&&cancelled.error.code==='cancelled','restart invalidates outstanding capture');
 check((await capture('after-cancel',true)).checksum===initial.checksum,'capture admission recovers after restart cancellation');
 const rejected=await request({type:'invoke',name:'teleport',arguments:{x:0,z:0}});
 check(rejected.status==='failure'&&rejected.error.code==='invalid_value','obstructed teleport reports failure');
 check((await capture('after-error')).checksum===initial.checksum,'capture stays fresh after rejected mutation');
 player.replay_route();player.resume();for(let i=0;i<44;i++)player.frame(1000/60+0.000001);
 let invalid=false;try{await BrowserPlayer.create(document.createElement('canvas'),'invalid');}catch{invalid=true;}check(invalid,'invalid backend reports an actionable error');
 if(timedOut)throw Error('browser GPU acceptance exceeded 60 seconds');
 publish({status:'passed',backend,checks,live,replay,final:state()});
} catch(error){publish({status:'failed',backend,error:String(error),checks});}
finally{clearTimeout(deadline);}
