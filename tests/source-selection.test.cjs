const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

function setup(source = null, fetchOverride = null) {
  const script = fs.readFileSync('src/app.js', 'utf8');
  const elements = {};
  for (const name of ['channels', 'search', 'status', 'source-form', 'source-url', 'source-user', 'source-password', 'source-file', 'source-status']) {
    elements['#' + name] = {value:'', files:[], textContent:'', listeners:{}, addEventListener(type, fn) {this.listeners[type]=fn}, replaceChildren(){}, append(){}};
  }
  Object.defineProperty(elements['#source-file'], 'value', {set(value){if(value==='')this.files=[]}, get(){return this.files.length?'selected':''}});
  elements.video = {listeners:{}, play:async()=>{}, addEventListener(type,fn){this.listeners[type]=fn}};
  const calls=[];
  elements['#channels'].children=[];
  elements['#channels'].replaceChildren=function(){this.children=[]};
  elements['#channels'].append=function(child){this.children.push(child)};
  const document={querySelector: selector=>elements[selector], createElement:()=>({children:[],listeners:{},addEventListener(type,fn){this.listeners[type]=fn},append(child){this.children.push(child)}})};
  const fetch=async (url,options)=>{calls.push({url,options}); return fetchOverride ? fetchOverride(url,options) : {ok:true,json:async()=>url==='/api/source'?source:[]}};
  vm.runInNewContext(script,{document,fetch,Error,JSON,TextDecoder});
  return {elements,calls};
}

test('a delayed initial channel failure does not overwrite playback of a replacement', async()=>{
  let rejectChannels;
  const {elements}=setup(null, url=>{
    if(url==='/api/channels') return new Promise((_,reject)=>{rejectChannels=reject});
    if(url==='/api/playlist') return {ok:true,json:async()=>[{name:'New',url:'https://example.org/live'}]};
    return {ok:true,json:async()=>null};
  });
  elements['#source-url'].value='https://example.org/list.m3u';
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  elements['#channels'].children[0].children[0].listeners.click();
  rejectChannels(new Error('offline'));
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(elements['#status'].textContent,'Playing: New');
});

test('reports an initial channel failure when no replacement has succeeded', async()=>{
  const {elements}=setup(null, url=>{
    if(url==='/api/channels') return {ok:false};
    return {ok:true,json:async()=>null};
  });
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(elements['#status'].textContent,'Could not load channels.');
});

test('a delayed source restoration failure does not overwrite a successful replacement', async()=>{
  let rejectSource;
  const {elements}=setup(null, url=>{
    if(url==='/api/source') return new Promise((_,reject)=>{rejectSource=reject});
    if(url==='/api/playlist') return {ok:true,json:async()=>[{name:'New',url:'https://example.org/live'}]};
    return {ok:true,json:async()=>[]};
  });
  elements['#source-url'].value='https://example.org/list.m3u';
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  rejectSource(new Error('offline'));
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(elements['#source-status'].textContent,'Loaded 1 channels.');
});

test('a delayed source restoration failure does not overwrite empty-form validation', async()=>{
  let rejectSource;
  const {elements}=setup(null, url=>{
    if(url==='/api/source') return new Promise((_,reject)=>{rejectSource=reject});
    return {ok:true,json:async()=>[{name:'Existing',url:'https://example.org/live'}]};
  });
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  rejectSource(new Error('offline'));
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(elements['#source-status'].textContent,'Choose a local file or enter a playlist URL.');
});

test('restores saved fields after an empty-form submission', async()=>{
  let resolveSource;
  const {elements}=setup(null, url=>{
    if(url==='/api/source') return new Promise(resolve=>{resolveSource=resolve});
    return {ok:true,json:async()=>[]};
  });
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  resolveSource({ok:true,json:async()=>({url:'https://example.org/list.m3u',username:'alice',password:'secret'})});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(elements['#source-url'].value,'https://example.org/list.m3u');
  assert.equal(elements['#source-user'].value,'alice');
  assert.equal(elements['#source-password'].value,'secret');
  assert.equal(elements['#source-status'].textContent,'Choose a local file or enter a playlist URL.');
});

test('a delayed empty channel response does not overwrite validation', async()=>{
  let resolveChannels;
  const {elements}=setup(null, url=>{
    if(url==='/api/channels') return new Promise(resolve=>{resolveChannels=resolve});
    return {ok:true,json:async()=>null};
  });
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  resolveChannels({ok:true,json:async()=>[]});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(elements['#source-status'].textContent,'Choose a local file or enter a playlist URL.');
});

test('an oversized-file warning survives a delayed source restoration failure', async()=>{
  let rejectSource;
  const {elements}=setup(null, url=>{
    if(url==='/api/source') return new Promise((_,reject)=>{rejectSource=reject});
    return {ok:true,json:async()=>[]};
  });
  elements['#source-file'].files=[{size:10*1024*1024+1}];
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  rejectSource(new Error('offline'));
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(elements['#source-status'].textContent,'Playlist exceeds 10 MiB.');
});

test('preserves initial channels when a replacement fails before they arrive', async()=>{
  let resolveChannels;
  const {elements}=setup(null, url=>{
    if(url==='/api/channels') return new Promise(resolve=>{resolveChannels=resolve});
    if(url==='/api/playlist') return {ok:false};
    return {ok:true,json:async()=>null};
  });
  elements['#source-url'].value='https://example.org/list.m3u';
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  resolveChannels({ok:true,json:async()=>[{name:'Existing channel',url:'https://example.org/stream'}]});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(elements['#channels'].children[0]?.children[0].textContent,'1. Existing channel');
  assert.match(elements['#source-status'].textContent,/could not load/i);
});

test('does not replace a successful new playlist with a delayed initial response', async()=>{
  let resolveChannels;
  const {elements}=setup(null, url=>{
    if(url==='/api/channels') return new Promise(resolve=>{resolveChannels=resolve});
    if(url==='/api/playlist') return {ok:true,json:async()=>[{name:'New channel',url:'https://example.org/new'}]};
    return {ok:true,json:async()=>null};
  });
  elements['#source-url'].value='https://example.org/list.m3u';
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  resolveChannels({ok:true,json:async()=>[{name:'Old channel',url:'https://example.org/old'}]});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(elements['#channels'].children[0]?.children[0].textContent,'1. New channel');
});

test('keeps the replacement error when a delayed initial playlist is empty', async()=>{
  let resolveChannels;
  const {elements}=setup(null, url=>{
    if(url==='/api/channels') return new Promise(resolve=>{resolveChannels=resolve});
    if(url==='/api/playlist') return {ok:false};
    return {ok:true,json:async()=>null};
  });
  elements['#source-url'].value='https://example.org/list.m3u';
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  resolveChannels({ok:true,json:async()=>[]});
  await new Promise(resolve=>setImmediate(resolve));
  assert.match(elements['#source-status'].textContent,/could not load/i);
});

test('restores the saved URL and credentials', async()=>{
  const {elements}=setup({url:'https://example.org/list.m3u',username:'alice',password:'secret'});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(elements['#source-url'].value,'https://example.org/list.m3u');
  assert.equal(elements['#source-user'].value,'alice');
  assert.equal(elements['#source-password'].value,'secret');
});

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
  elements['#source-file'].files=[{size:30,arrayBuffer:()=>new Promise(resolve=>{finish=resolve})}];
  const first=elements['#source-form'].listeners.submit({preventDefault(){}});
  elements['#source-file'].files=[];
  elements['#source-url'].value='https://example.org/list.m3u';
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  finish(Buffer.from('#EXTM3U\n'));
  await first;
  const requests=calls.filter(c=>c.url==='/api/playlist');
  assert.equal(requests.length,1);
  assert.deepEqual(JSON.parse(requests[0].options.body),{content:'#EXTM3U\n'});
});

test('loads a URL after a successful local file upload', async()=>{
  const {elements,calls}=setup();
  elements['#source-file'].files=[{size:8,arrayBuffer:async()=>Buffer.from('#EXTM3U\n')}];
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  elements['#source-url'].value='https://example.org/list.m3u';
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  const requests=calls.filter(c=>c.url==='/api/playlist');
  assert.equal(requests.length,2);
  assert.deepEqual(JSON.parse(requests[1].options.body),{url:'https://example.org/list.m3u'});
});

test('preserves a new file selected during an upload', async()=>{
  const {elements}=setup();
  let finish;
  const firstFile={size:8,arrayBuffer:()=>new Promise(resolve=>{finish=resolve})};
  const nextFile={size:8,arrayBuffer:async()=>Buffer.from('#EXTM3U\n')};
  elements['#source-file'].files=[firstFile];
  const pending=elements['#source-form'].listeners.submit({preventDefault(){}});
  elements['#source-file'].files=[nextFile];
  finish(Buffer.from('#EXTM3U\n'));
  await pending;
  assert.equal(elements['#source-file'].files[0],nextFile);
});

test('preserves URL and credentials edited while a local upload is pending', async()=>{
  const {elements}=setup();
  let finish;
  elements['#source-file'].files=[{size:8,arrayBuffer:()=>new Promise(resolve=>{finish=resolve})}];
  const pending=elements['#source-form'].listeners.submit({preventDefault(){}});
  for(const [name,value] of [['source-url','https://example.org/new.m3u'],['source-user','alice'],['source-password','secret']]){
    elements['#'+name].value=value;
    elements['#'+name].listeners.input();
  }
  finish(Buffer.from('#EXTM3U\n'));
  await pending;
  assert.equal(elements['#source-url'].value,'https://example.org/new.m3u');
  assert.equal(elements['#source-user'].value,'alice');
  assert.equal(elements['#source-password'].value,'secret');
});

test('clears stale credentials when only the URL changes during a local upload', async()=>{
  const {elements,calls}=setup();
  let finish;
  elements['#source-url'].value='https://old.example/list.m3u';
  elements['#source-user'].value='olduser';
  elements['#source-password'].value='oldsecret';
  elements['#source-file'].files=[{size:8,arrayBuffer:()=>new Promise(resolve=>{finish=resolve})}];
  const pending=elements['#source-form'].listeners.submit({preventDefault(){}});
  elements['#source-url'].value='https://new.example/list.m3u';
  elements['#source-url'].listeners.input();
  finish(Buffer.from('#EXTM3U\n'));
  await pending;
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  const requests=calls.filter(c=>c.url==='/api/playlist');
  assert.deepEqual(JSON.parse(requests[1].options.body),{url:'https://new.example/list.m3u'});
});

test('uploads the chosen local file contents', async()=>{
  const {elements,calls}=setup();
  elements['#source-file'].files=[new Blob(['#EXTM3U\n#EXTINF:-1,Café\nhttps://example.org/live\n'])];
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  const request=calls.find(c=>c.url==='/api/playlist');
  assert.deepEqual(JSON.parse(request.options.body),{content:'#EXTM3U\n#EXTINF:-1,Café\nhttps://example.org/live\n'});
});

test('rejects invalid UTF-8 without replacing the playlist', async()=>{
  const {elements,calls}=setup();
  elements['#source-file'].files=[new Blob([Buffer.from([0x23,0x45,0x58,0x54,0x4d,0x33,0x55,0x0a,0xff])])];
  await elements['#source-form'].listeners.submit({preventDefault(){}});
  assert.equal(calls.filter(c=>c.url==='/api/playlist').length,0);
  assert.match(elements['#source-status'].textContent,/utf-8/i);
  assert.equal(elements['#source-file'].files.length,1);
});
