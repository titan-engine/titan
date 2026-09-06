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
 link.download=`adventure-${backend}-evidence.json`;evidence.prepend(link);
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
 await capture('start');
 player.set_control_enabled(true);
 const sequenceBindings={up:'ArrowUp',down:'ArrowDown',left:'ArrowLeft',right:'ArrowRight',jump:'Space',switch:'KeyQ',interact:'KeyE',confirm:'Enter',restart:'KeyR'};
 const sequenceRoute=await (await fetch('sequence-solution.json')).json();
 player.resume();
 for(const segment of sequenceRoute){
   for(const [action,key] of Object.entries(sequenceBindings))player.set_key(key,segment.actions.includes(action),false);
   for(let i=0;i<segment.ticks;i++)player.frame(1000/60+0.000001);
   if(segment.checkpoint==='started'||segment.checkpoint==='continued'){
     check(state().phase==='playing'&&state().session_tick===0&&state().active_character==='jumper',`sequence ${segment.checkpoint}: fresh Jumper room`);
     await capture(`sequence-${segment.checkpoint}`);
   } else if(segment.checkpoint==='complete') {
     check(state().phase===(state().room===1?'room_complete':'slice_complete'),'sequence completion requires explicit confirmation');
     await capture(`sequence-room-${state().room}-complete`);
   }
 }
 const sequenceSolved=state(),sequenceRecording=player.recording();
 check(sequenceSolved.phase==='slice_complete','keyboard start-to-finish sequence completes');
 player.set_key('Enter',true,false);player.frame(1000/60+0.000001);player.set_key('Enter',false,false);
 check(state().room===1&&state().phase==='playing'&&state().session_tick===0,'Play again starts room 1');
 await capture('sequence-play-again');
 player.load_recording(sequenceRecording);player.resume();
 for(let i=0;i<JSON.parse(sequenceRecording).frames.length;i++)player.frame(1000/60+0.000001);
 check(state().phase==='slice_complete'&&JSON.stringify(state().characters)===JSON.stringify(sequenceSolved.characters),'full sequence GPU replay preserves transitions');
 await capture('sequence-replay');
 player.restart();
 check(state().room===2&&state().phase==='playing'&&state().session_tick===0,'Restart room at final completion keeps displayed room');
 await capture('sequence-restart-room');
 for(const key of Object.values(sequenceBindings))player.set_key(key,false,false);
 player.select_room(1);
 const initial=await capture('initial');
 check((await capture('initial-repeat')).checksum===initial.checksum,'read-only repeated capture pixels stable');
 player.set_control_enabled(true);

 const tick=()=>player.frame(1000/60+0.000001);
 player.resume();
 player.set_key('KeyD',true,false);
 for(let i=0;i<8;i++)tick();
 player.set_key('KeyD',false,false);
 check(state().characters.jumper.x===1980,'eight fixed movement ticks move Jumper');
 const moved=await capture('moved');
 player.set_key('KeyQ',true,false);tick();player.set_key('KeyQ',false,false);
 check(state().active_character==='strong','Q selects Strong');
 const switched=await capture('switched');
 check(switched.checksum!==moved.checksum,'switch visibly changes active marker');
 player.set_key('ArrowUp',true,false);for(let i=0;i<8;i++)tick();player.set_key('ArrowUp',false,false);
 player.pause();
 const live=state();
 check(live.session_tick===17&&live.characters.strong.z===6020,'17-tick keyboard route matches semantic foundation');
 const finish=await capture('route');
 const recording=player.recording();
 player.load_recording(recording);player.resume();for(let i=0;i<17;i++)tick();
 const replay=state();
 check(JSON.stringify(replay.characters)===JSON.stringify(live.characters)&&replay.active_character===live.active_character&&player.paused(),'recorded keyboard route replays exactly and pauses');
 check((await capture('replay')).checksum===finish.checksum,'replay pixels match live route');
 player.resize(0,0);check(!player.frame(0)&&state().surface.suspended,'zero-sized canvas suspends presentation');
 check((await capture('suspended')).checksum===finish.checksum,'offscreen capture survives suspended surface');
 for(const [width,height] of [[640,360],[800,500],[1280,720],[960,540]]) {
   player.resize(width,height);check(player.frame(0)&&!state().surface.suspended,`${width}x${height} presents`);
 }
 player.resize(2560,1440);
 check(player.frame(0)&&JSON.stringify(state().surface.size)===JSON.stringify([2048,1152]),'high-DPI backing size preserves 16:9 under allocation cap');
 player.resize(960,540);
 player.restart();player.resume();
 player.set_key('KeyD',true,false);player.set_key('ArrowRight',true,false);tick();
 player.set_key('KeyQ',true,false);tick();player.set_key('KeyQ',false,false);
 player.set_key('KeyD',false,false);tick();
 check(state().characters.strong.x===3500,'switch suppresses held physical aliases at logical action level');
 player.set_key('ArrowRight',false,false);tick();player.set_key('KeyD',true,false);tick();
 check(state().characters.strong.x===3560,'fresh movement controls selected character');
 player.set_key('KeyQ',true,false);tick();player.set_key('KeyQ',false,false);
 player.set_key('KeyD',false,false);player.set_key('KeyD',true,false);tick();
 check(state().characters.jumper.x===1620,'release/repress between ticks unlocks the selected action');
 player.set_key('KeyQ',true,false);tick();player.set_key('KeyQ',false,false);
 player.set_key('KeyD',false,false);player.set_key('KeyD',true,false);tick();
 check(state().characters.strong.x===3620,'quick release/repress survives a second switch');
 player.pause();const pausedTick=state().session_tick;player.frame(100);
 check(state().session_tick===pausedTick,'pause freezes simulation ticks');
 player.resume();player.set_key('KeyD',true,true);tick();
 check(state().characters.strong.x===3620,'resume discards stale held movement');
 player.set_key('KeyD',false,false);player.set_key('KeyD',true,false);tick();
 check(state().characters.strong.x===3680,'release and repress restores movement after pause');
 player.set_key('KeyR',true,false);tick();player.set_key('KeyR',false,false);
 check(state().characters.jumper.x===1500&&state().characters.strong.x===3500&&state().active_character==='jumper','R reconstructs both character starts');
 player.set_key('KeyD',false,false);player.set_key('ArrowUp',true,false);tick();player.set_key('ArrowUp',false,false);
 const afterRestart=state();const restartRecording=player.recording();
 player.load_recording(restartRecording);
 check((await request({type:'step',frames:1})).status==='success'&&state().playback.active&&state().playback.position===1,'inspector step retains replay after recorded restart');
 check((await request({type:'step',frames:1})).status==='success'&&state().playback.complete&&JSON.stringify(state().characters)===JSON.stringify(afterRestart.characters),'inspector replay continues after recorded restart and matches live state');
 player.pause();
 const pending=request({type:'capture'});const overlapping=request({type:'capture'});player.restart();
 const busy=await overlapping;check(busy.status==='failure'&&busy.error.code==='busy','overlapping capture is bounded busy');
 const cancelled=await pending;check(cancelled.status==='failure'&&cancelled.error.code==='cancelled','restart invalidates outstanding capture');
 const reset=await capture('reset',true);
 check(reset.checksum===initial.checksum&&reset.identity.session_generation>initial.identity.session_generation,'reset restores initial pixels and advances capture generation');
 // Exercise visible support geometry using ordinary keyboard input only.
 player.resume();
 player.set_key('ArrowUp',true,false);for(let i=0;i<50;i++)tick();player.set_key('ArrowUp',false,false);
 check(state().characters.jumper.z===3500,'approach teaching ledge on floor');
 player.set_key('Space',true,false);player.set_key('ArrowUp',true,false);
 for(let i=0;i<17;i++)tick();
 check(state().characters.jumper.y===1530,'Jumper reaches selected 1.53m apex');
 await capture('jump-apex');
 for(let i=0;i<5;i++)tick();player.set_key('ArrowUp',false,false);player.set_key('Space',false,false);
 for(let i=0;i<14;i++)tick();
 check(state().characters.jumper.y===1000&&state().characters.jumper.grounded,'Jumper lands on visible teaching ledge');
 await capture('ledge-landed');
 player.set_key('ArrowDown',true,false);for(let i=0;i<30;i++)tick();player.set_key('ArrowDown',false,false);
 for(let i=0;i<20;i++)tick();
 check(state().characters.jumper.y===0&&state().characters.jumper.grounded,'walking off ledge lands safely');
 player.restart();player.resume();
 player.set_key('KeyQ',true,false);tick();player.set_key('KeyQ',false,false);
 player.set_key('ArrowLeft',true,false);for(let i=0;i<25;i++)tick();player.set_key('ArrowLeft',false,false);
 player.set_key('ArrowUp',true,false);for(let i=0;i<50;i++)tick();player.set_key('ArrowUp',false,false);
 player.set_key('Space',true,false);player.set_key('ArrowUp',true,false);for(let i=0;i<9;i++)tick();
 check(state().characters.strong.y===450,'Strong reaches selected 0.45m apex');
 await capture('strong-apex');
 for(let i=0;i<31;i++)tick();player.set_key('Space',false,false);player.set_key('ArrowUp',false,false);
 check(state().characters.strong.y===0&&state().characters.strong.z===3200,'Strong cannot mount 1m ledge and held Space does not repeat');
 await capture('strong-blocked');player.pause();
 // Play the complete cooperative route on this same real GPU/WASM player.
 player.restart();player.resume();
 const solution=await (await fetch('puzzle-solution.json')).json();
 const bindings={up:'ArrowUp',down:'ArrowDown',left:'ArrowLeft',right:'ArrowRight',jump:'Space',switch:'KeyQ'};
 for(const segment of solution){
   for(const [action,key] of Object.entries(bindings))player.set_key(key,segment.actions.includes(action),false);
   for(let i=0;i<segment.ticks;i++)tick();
   if(segment.checkpoint){
     const puzzle=state().puzzle;
     if(segment.checkpoint==='plate-a')check(puzzle.plates[0].pressed&&puzzle.door.state==='open_plate','raised plate opens door');
     if(segment.checkpoint==='plate-b')check(puzzle.plates.every(p=>p.pressed),'inactive Jumper holds A while Strong reaches B');
     if(segment.checkpoint==='exchange')check(!puzzle.plates[0].pressed&&puzzle.plates[1].pressed&&puzzle.door.open,'far plate holds passage after Jumper leaves ledge');
     if(segment.checkpoint==='jumper-exit')check(puzzle.exit.jumper&&!puzzle.exit.strong&&!puzzle.complete,'one complete exit footprint does not finish');
     if(segment.checkpoint==='complete')check(puzzle.complete&&puzzle.exit.jumper&&puzzle.exit.strong&&puzzle.door.state==='closed','both grounded exit footprints complete room and door closes');
     await capture(`puzzle-${segment.checkpoint}`);
   }
 }
 for(const key of Object.values(bindings))player.set_key(key,false,false);
 const solved=state(),solvedRecording=player.recording();
 player.set_key('ArrowLeft',true,false);player.set_key('Space',true,false);player.set_key('KeyQ',true,false);
 for(let i=0;i<10;i++)tick();
 check(state().session_tick===solved.session_tick&&JSON.stringify(state().characters)===JSON.stringify(solved.characters)&&state().active_character===solved.active_character,'completion freezes movement, switching and puzzle clock');
 player.load_recording(solvedRecording);player.resume();
 for(let i=0;i<JSON.parse(solvedRecording).frames.length;i++)tick();
 check(state().puzzle.complete&&JSON.stringify(state().characters)===JSON.stringify(solved.characters)&&player.paused(),'complete solution recording replays in actual GPU player');
 await capture('puzzle-solution-replay');
 for(const [width,height] of [[1280,720],[960,540]]){player.resize(width,height);check(player.frame(0),`completed room presents at ${width}x${height}`);}
 player.restart();
 check(!state().puzzle.complete&&!state().puzzle.door.open&&state().puzzle.plates.every(p=>!p.pressed),'restart after completion reconstructs all devices');
 await capture('puzzle-restarted');
 player.resume();
 const obstruction=[...solution.slice(0,4),{actions:['switch'],ticks:1},{actions:[],ticks:1},{actions:['up'],ticks:25},{actions:['right'],ticks:67},{actions:['switch'],ticks:1},{actions:[],ticks:1},{actions:['down'],ticks:50}];
 for(const segment of obstruction){
   for(const [action,key] of Object.entries(bindings))player.set_key(key,segment.actions.includes(action),false);
   for(let i=0;i<segment.ticks;i++)tick();
 }
 check(state().puzzle.door.state==='open_obstructed','inactive body safely holds unrequested door open');
 await capture('puzzle-obstructed');
 for(const segment of [{actions:['switch'],ticks:1},{actions:[],ticks:1},{actions:['right'],ticks:15}]){
   for(const [action,key] of Object.entries(bindings))player.set_key(key,segment.actions.includes(action),false);
   for(let i=0;i<segment.ticks;i++)tick();
 }
 check(state().puzzle.door.state==='closed','door closes after obstructing body clears');
 await capture('puzzle-cleared');player.pause();
 // Practice remains available to isolate both physical block routes.
 player.select_room(2); player.resume();
 player.set_key('KeyE',true,false); player.set_key('ArrowUp',true,false); tick();
 check(state().block.last_rejection==='wrong_character'&&state().block.socket===0,'Jumper push visibly rejected without moving block');
 await capture('block-rejected'); player.pause();
 for (const routeName of ['block-solution.json','block-intermediate-solution.json']) {
   player.select_room(2); player.resume();
   check(state().room===2,'practice selector constructs room 2');
   await capture(`block-${routeName}-initial`);
   const route=await (await fetch(routeName)).json();
   const blockBindings={...bindings,interact:'KeyE',restart:'KeyR'};
   for(const segment of route){
     for(const [action,key] of Object.entries(blockBindings))player.set_key(key,segment.actions.includes(action),false);
     for(let i=0;i<segment.ticks;i++)tick();
     if(segment.checkpoint)await capture(`${routeName}-${segment.checkpoint}`);
   }
   check(state().puzzle.complete,`${routeName}: ordinary keyboard input completes room 2`);
   const solvedBlock=state(), blockRecording=player.recording();
   player.load_recording(blockRecording); player.resume();
   for(let i=0;i<JSON.parse(blockRecording).frames.length;i++)tick();
   check(state().room===2&&state().puzzle.complete&&JSON.stringify(state().characters)===JSON.stringify(solvedBlock.characters),`${routeName}: room-aware player replay matches`);
   player.restart();
   check(state().room===2&&!state().puzzle.complete,`${routeName}: restart keeps room 2`);
 }
 await capture('block-reset'); player.pause();
 let invalid=false;try{await BrowserPlayer.create(document.createElement('canvas'),'invalid');}catch{invalid=true;}check(invalid,'invalid backend reports an actionable error');
 if(timedOut)throw Error('browser GPU acceptance exceeded 60 seconds');
 publish({status:'passed',backend,checks,live,replay,final:state()});
} catch(error){publish({status:'failed',backend,error:String(error),checks});}
finally{clearTimeout(deadline);}
