import test from 'node:test';
import assert from 'node:assert/strict';
import { logicalPointer } from './pointer.mjs';
test('CSS resize, offset and nonuniform scale retain tile coordinates irrespective of DPR', () => {
  for (const [width,height] of [[384,256],[1152,768],[720,420]]) {
    const rect={left:27,top:53,width,height};
    const point=logicalPointer(27+80/384*width,53+112/256*height,rect);
    assert.equal(Math.floor(point.x/32),2);
    assert.equal(Math.floor(point.y/32),3);
  }
});
test('outside canvas remains outside so validation cannot place an edge tile', () => {
  const rect={left:10,top:10,width:384,height:256};
  assert.equal(logicalPointer(394,30,rect).x,384);
  assert.equal(logicalPointer(9,30,rect).x,-1);
});
