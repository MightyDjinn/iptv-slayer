const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

function setup() {
  const html = fs.readFileSync('src/index.html', 'utf8');
  const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];
  const elements = {};
  for (const name of ['channels', 'search', 'status', 'source-form', 'source-url', 'source-user', 'source-password', 'source-file', 'source-status']) {
    elements['#' + name] = {value:'', files:[], textContent:'', listeners:{}, addEventListener(type, fn) {this.listeners[type]=fn}, replaceChildren(){}, append(){}};
  }
  elements.video = {listeners:{}, addEventListener(type,fn){this.listeners[type]=fn}};
  const calls=[];
  const document={querySelector: selector=>elements[selector], createElement:()=>({addEventListener(){},append(){}})};
  const fetch=async (url,options)=>{calls.push({url,options}); return {ok:true,json:async()=>[]}};
  vm.runInNewContext(script,{document,fetch,Error,JSON});
  return {elements,calls};
}

test('prompts for a playlist when the site starts empty', async()=>{
  const {elements}=setup();
  await new Promise(resolve=>setImmediate(resolve));
  assert.match(elements['#source-status'].textContent,/choose.*playlist/i);
});

test('submits authenticated URL without putting credentials in URL', async()=>{
  const {elements,calls}=setup();
  elements['#source-url'].value='https://example.org/list.m3u';
  elements['#source-user'].value='alice'; elements['#source-password'].value='secret';
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  const request=calls.find(c=>c.url==='/api/playlist');
  assert.equal(request.options.method,'POST');
  assert.deepEqual(JSON.parse(request.options.body),{url:'https://example.org/list.m3u',username:'alice',password:'secret'});
});

test('submits an unauthenticated URL without empty credential fields', async()=>{
  const {elements,calls}=setup();
  elements['#source-url'].value='https://example.org/list.m3u';
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  const request=calls.find(c=>c.url==='/api/playlist');
  assert.deepEqual(JSON.parse(request.options.body),{url:'https://example.org/list.m3u'});
});

test('ignores a second source submission while the first is being read', async()=>{
  const {elements,calls}=setup();
  let finish;
  elements['#source-file'].files=[{size:30,text:()=>new Promise(resolve=>{finish=resolve})}];
  const first=elements['#source-form'].listeners.submit({preventDefault(){}});
  elements['#source-file'].files=[];
  elements['#source-url'].value='https://example.org/list.m3u';
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  finish('#EXTM3U\n');
  await first;
  const requests=calls.filter(c=>c.url==='/api/playlist');
  assert.equal(requests.length,1);
  assert.deepEqual(JSON.parse(requests[0].options.body),{content:'#EXTM3U\n'});
});

test('uploads the chosen local file contents', async()=>{
  const {elements,calls}=setup();
  elements['#source-file'].files=[{size:30,text:async()=>'#EXTM3U\n'}];
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  const request=calls.find(c=>c.url==='/api/playlist');
  assert.deepEqual(JSON.parse(request.options.body),{content:'#EXTM3U\n'});
});
