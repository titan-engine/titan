import test from 'node:test';
import assert from 'node:assert/strict';
import {bindKeys} from './keys.mjs';
class Events { handlers={}; addEventListener(type,fn){(this.handlers[type]??=[]).push(fn);} emit(type,event={}){for(const fn of this.handlers[type]??[]) fn({preventDefault(){},...event});} }
test('physical aliases and normal release survive focus outside canvas; blur cancels orphan release',()=>{
 const window=new Events(),document=new Events(),canvas={},other={},calls=[];
 bindKeys({canvas,window,document,key:(...a)=>calls.push(a),clear:()=>calls.push('clear'),shortcut:()=>false});
 window.emit('keydown',{target:other,code:'KeyW'});assert.deepEqual(calls,[]);
 window.emit('keydown',{target:canvas,code:'KeyW',repeat:false});
 window.emit('keydown',{target:canvas,code:'ArrowUp',repeat:false});
 window.emit('keyup',{target:other,code:'KeyW'});
 assert.deepEqual(calls,[['KeyW',true,false],['ArrowUp',true,false],['KeyW',false,false]]);
 window.emit('blur');window.emit('keyup',{code:'ArrowUp'});assert.deepEqual(calls.at(-1),['ArrowUp',false,false]);assert.equal(calls.length,5);
 document.emit('focusin',{target:other});assert.equal(calls.length,6);
});
test('shortcuts only run on canvas and do not repeat',()=>{
 const window=new Events(),document=new Events(),canvas={},calls=[];
 bindKeys({canvas,window,document,key:()=>{},clear:()=>{},shortcut:code=>{calls.push(code);return true;}});
 window.emit('keydown',{target:{},code:'Space'});window.emit('keydown',{target:canvas,code:'Space',repeat:false});window.emit('keydown',{target:canvas,code:'Space',repeat:true});
 assert.deepEqual(calls,['Space']);
});

test('focus loss pauses; Q, R and Space use gameplay input',()=>{
 const window=new Events(),document=new Events(),canvas={},calls=[];
 bindKeys({canvas,window,document,key:(...a)=>calls.push(a),clear:()=>calls.push('clear'),pause:()=>calls.push('pause'),shortcut:()=>false});
 for(const code of ['KeyQ','KeyR','Space']) window.emit('keydown',{target:canvas,code,repeat:false});
 assert.deepEqual(calls,[['KeyQ',true,false],['KeyR',true,false],['Space',true,false]]);
 document.emit('focusin',{target:{}});assert.deepEqual(calls.slice(-2),['clear','pause']);
});
