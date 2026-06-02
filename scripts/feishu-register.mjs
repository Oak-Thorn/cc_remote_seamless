import * as lark from '@larksuiteoapi/node-sdk';

async function main() {
  try {
    const result = await lark.registerApp({
      source: 'cc-remote-seamless',
      onQRCodeReady(info) {
        console.log(JSON.stringify({ type: 'qr', url: info.url, expireIn: info.expireIn }));
      },
      onStatusChange(info) {
        console.log(JSON.stringify({ type: 'status', status: info.status }));
      },
    });

    console.log(JSON.stringify({
      type: 'done',
      client_id: result.client_id,
      client_secret: result.client_secret,
    }));
  } catch (e) {
    console.log(JSON.stringify({
      type: 'error',
      code: e.code || 'unknown',
      description: e.description || e.message || 'Unknown error',
    }));
    process.exit(1);
  }
}

main();
