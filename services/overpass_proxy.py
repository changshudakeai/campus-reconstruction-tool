import http.server, urllib.request, urllib.parse

class H(http.server.BaseHTTPRequestHandler):
 def do_POST(s):
  n=int(s.headers.get('Content-Length',0))
  p=urllib.parse.parse_qs(s.rfile.read(n).decode())
  q=p.get('data',[''])[0]
  r=urllib.request.urlopen(urllib.request.Request('https://overpass.kumi.systems/api/interpreter',urllib.parse.urlencode({'data':q}).encode(),{'Content-Type':'application/x-www-form-urlencoded'}),timeout=35)
  c=r.read()
  s.send_response(200)
  s.send_header('Content-Type','application/json')
  s.send_header('Access-Control-Allow-Origin','*')
  s.end_headers()
  s.wfile.write(c)
 def do_OPTIONS(s):
  s.send_response(204)
  s.send_header('Access-Control-Allow-Origin','*')
  s.send_header('Access-Control-Allow-Methods','POST, OPTIONS')
  s.send_header('Access-Control-Allow-Headers','Content-Type')
  s.end_headers()

http.server.HTTPServer(('127.0.0.1',9090),H).serve_forever()