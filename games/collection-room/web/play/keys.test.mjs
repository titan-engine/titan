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
 window.emit('blur');window.emit('keyup',{code:'ArrowUp'});assert.equal(calls.at(-1),'clear');assert.equal(calls.length,4);
 document.emit('focusin',{target:other});assert.equal(calls.length,5);
});
test('shortcuts only run on canvas and do not repeat',()=>{
 const window=new Events(),document=new Events(),canvas={},calls=[];
 bindKeys({canvas,window,document,key:()=>{},clear:()=>{},shortcut:code=>{calls.push(code);return true;}});
 window.emit('keydown',{target:{},code:'Space'});window.emit('keydown',{target:canvas,code:'Space',repeat:false});window.emit('keydown',{target:canvas,code:'Space',repeat:true});
 assert.deepEqual(calls,['Space']);
});
