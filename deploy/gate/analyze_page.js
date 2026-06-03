export const ANALYZE_HTML = `<!doctype html><meta charset=utf-8><title>Analyze · yousummary</title>
<style>body{font-family:system-ui;max-width:920px;margin:2rem auto;padding:0 1rem;color:#111}
label{display:block;margin-top:1rem;font-weight:600}
input,textarea,select{width:100%;padding:.5rem;box-sizing:border-box;font:inherit}
textarea{min-height:90px;font:14px ui-monospace,Menlo,monospace}
.row{display:flex;gap:1rem}.row>div{flex:1}
button{margin-top:1rem;padding:.6rem 1.2rem;font-size:1rem}
.muted{color:#666;font-size:.9rem}.err{color:tomato;white-space:pre-wrap}
#out{margin-top:1.5rem}.spinner{display:inline-block;width:1em;height:1em;border:2px solid #ccc;border-top-color:#222;border-radius:50%;animation:spin .8s linear infinite;vertical-align:-.15em}@keyframes spin{to{transform:rotate(360deg)}}
table{border-collapse:collapse;width:100%}th,td{border:1px solid #ddd;padding:.5rem .7rem;text-align:left;vertical-align:top}th{background:#f5f5f5}</style>
<h1>Analyze videos</h1>
<p class=muted>Add one or more YouTube URLs (one per line) and/or paste a transcript. Several URLs → ranked comparison.</p>
<form id=f>
<label>YouTube URL(s)<textarea id=urls placeholder="https://www.youtube.com/watch?v=...&#10;https://youtu.be/..."></textarea></label>
<div class=row>
<div><label>Mode<select id=mode>
<option value=auto>Auto</option><option value=summary>Summary</option><option value=tutorial>Tutorial (verify commands)</option><option value=compare-extract>Comparison-extract</option><option value=rank>Rank multiple</option><option value=research>Research / fact-check</option><option value=product-score>Product scoring</option></select></div>
<div><label>Depth<select id=depth><option value=quick>Quick</option><option value=medium selected>Medium</option><option value=comprehensive>Comprehensive</option></select></div>
</div>
<label>What are you looking for? (optional)<input id=intent placeholder="e.g. set up X on Windows"></label>
<label>Paste transcript (optional — bypasses URL fetch)<textarea id=transcript placeholder="paste a transcript here to skip YouTube fetching"></textarea></label>
<label>Custom instructions (optional — overrides Mode/Depth)<textarea id=custom placeholder="free-form instructions for one-off asks"></textarea></label>
<button type=submit id=go>Analyze</button> <span id=status class=muted></span>
</form>
<div id=out></div>
<script>
const $=id=>document.getElementById(id);let timer=null;
async function poll(id){try{const r=await fetch('/api/analyze/'+id);const j=await r.json();
if(j.status==='done'){clearInterval(timer);$('status').textContent='done (#'+id+')';$('out').innerHTML=j.result_html||'<p class=muted>empty</p>';$('go').disabled=false;}
else if(j.status==='failed'){clearInterval(timer);$('status').innerHTML='<span class=err>failed: '+(j.error||'')+'</span>';$('go').disabled=false;}
else{$('status').innerHTML='<span class=spinner></span> #'+id+' '+j.status+'…';}}catch(e){}}
$('f').onsubmit=async ev=>{ev.preventDefault();$('out').innerHTML='';$('go').disabled=true;$('status').innerHTML='<span class=spinner></span> submitting…';
const urls=$('urls').value.split(/\\r?\\n/).map(s=>s.trim()).filter(Boolean);
const payload={urls,mode:$('mode').value,depth:$('depth').value,intent:$('intent').value||null,transcript:$('transcript').value||null,custom:$('custom').value||null};
try{const r=await fetch('/api/analyze',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(payload)});
const j=await r.json();if(!r.ok){$('status').innerHTML='<span class=err>'+(j.detail||r.status)+'</span>';$('go').disabled=false;return;}
$('status').innerHTML='<span class=spinner></span> queued #'+j.id+'…';poll(j.id);timer=setInterval(()=>poll(j.id),3000);}
catch(e){$('status').innerHTML='<span class=err>'+e.message+'</span>';$('go').disabled=false;}};
</script>`;
