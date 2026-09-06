import test from 'node:test';
import assert from 'node:assert/strict';
import {exampleArguments,parseArguments} from './commands.mjs';
test('tagged command arguments include only fields used by the selected variant',()=>{
 assert.deepEqual(parseArguments(JSON.stringify(exampleArguments('construct'))),{op:'place',kind:'conveyor',x:2,y:3,facing:'E'});
 assert.deepEqual(parseArguments('{"op":"rotate","x":2,"y":3}'),{op:'rotate',x:2,y:3});
 assert.deepEqual(parseArguments('{"kind":"processor"}'),{kind:'processor'});
 assert.deepEqual(parseArguments('{"facing":"W"}'),{facing:'W'});
 for(const text of ['null','[]','"value"','1']) assert.throws(()=>parseArguments(text));
});
