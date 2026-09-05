import init, {BrowserPlayer} from '../inspector/pkg/titan_game.js';
const result=document.getElementById('result');
const backend=new URL(location.href).searchParams.get('backend')??'webgpu';
const checks=[];
const check=(value,message)=>{if(!value)throw Error(message);checks.push(message);};
const canvas=document.querySelector('canvas');
let player, timedOut=false;
const deadline=setTimeout(()=>{timedOut=true;result.textContent=JSON.stringify({status:'failed',backend,error:'60-second browser deadline expired; reload to discard session',checks});},60000);
try {
 await init(); player=await BrowserPlayer.create(canvas,backend);
 const state=()=>JSON.parse(player.status());
 const request=async request=>JSON.parse(await player.dispatch(JSON.stringify({schema_version:2,request_id:`gpu-${checks.length}`,request})));
 check(player.frame(0),'initial GPU frame presented');
 check((await request({type:'step',frames:1})).error.code==='mutation_disabled','browser controls default disabled');
 player.set_control_enabled(true);
 check((await request({type:'capture'})).error.code==='unsupported','capture remains unregistered');
 player.resume();
 for(const [key,ticks] of [['ArrowRight',8],['ArrowUp',20],['ArrowRight',16]]) {
   player.set_key(key,true,false);
   for(let tick=0;tick<ticks;tick++) player.frame(1000/60+0.000001);
   player.set_key(key,false,false);
 }
 player.pause();
 const live=state();
 check(live.session_tick===44&&live.collected===3&&live.completed&&live.position.x===3000&&live.position.z===-2000,'44-tick keyboard route matches headless semantics');
 const recording=player.recording();
 player.load_recording(recording);player.resume();
 for(let tick=0;tick<44;tick++)player.frame(1000/60+0.000001);
 const replay=state();
 check(replay.session_tick===44&&replay.collected===3&&replay.completed&&replay.position.x===3000&&replay.position.z===-2000&&player.paused(),'interactive recording replay completes at exactly 44 ticks and pauses');
 player.resize(0,0);check(!player.frame(0)&&state().surface.suspended,'zero-sized canvas suspends presentation');
 player.resize(640,360);check(player.frame(0)&&!state().surface.suspended,'resize restores GPU presentation');
 player.resize(960,540);player.frame(0);
 player.restart();player.resume();player.set_key('KeyD',true,false);player.set_key('KeyD',false,false);player.frame(1000/60+0.000001);player.pause();
 check(state().position.x===-2750,'tap released before a tick still moves once');
 player.restart();player.resume();player.set_key('KeyD',true,false);player.clear_input();player.frame(1000/60+0.000001);player.pause();
 check(state().position.x===-3000,'focus cancellation discards held input and taps');
 player.replay_route();player.step();check(state().session_tick===1&&player.paused(),'paused replay step consumes one tick');
 player.restart();check(state().session_tick===0&&state().collected===0&&!state().playback.active,'restart clears progress and playback');
 player.replay_route();player.resume();for(let i=0;i<44;i++)player.frame(1000/60+0.000001);
 let invalid=false;try{await BrowserPlayer.create(document.createElement('canvas'),'invalid');}catch{invalid=true;}check(invalid,'invalid backend reports an actionable error');
 if(timedOut)throw Error('browser GPU acceptance exceeded 60 seconds');
 result.textContent=JSON.stringify({status:'passed',backend,checks,live,replay,final:state()},null,2);
} catch(error){result.textContent=JSON.stringify({status:'failed',backend,error:String(error),checks},null,2);}
finally{clearTimeout(deadline);}
