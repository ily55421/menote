const {App, Logger} = require('jj.js');

// 忽略客户端提前断开连接的错误：
//   - ERR_STREAM_PREMATURE_CLOSE：手机端 QUIC 流半关闭时 http.Server 的正常反应
//   - P2P 传输层 NAPI 错误（ClosedStream/GenericFailure 等）：手机断网/切网时
//     iroh 流关闭的连锁反应，lib/p2p.js 的 shim 已本地消化，此处兜底防逃逸。
//     不匹配的未知错误仍向上抛（保持对真 bug 的可见性）。
process.on('uncaughtException', (err) => {
    const known = err.code === 'ERR_STREAM_PREMATURE_CLOSE'
        || /ClosedStream|StreamClosed|LocallyClosed|stopped|closed stream|unknown handle/i
            .test(String(err?.message || err));
    if(known) {
        Logger.warning('[p2p] 忽略传输层断开错误: ' + (err.message || err));
        return;
    }
    throw err;
});

// server
const port = 3107;

// 响应压缩（gzip/br/deflate）：前端首屏需拉 element-plus(1MB)、vditor(290KB)、
// admin.js(138KB) 等未压缩资源，Brotli 后传输量降约 75%。
// 必须通过 App 的 middleware 选项注入——jj.js 在构造函数里就注册了 koa-static，
// 构造之后再 app.use() 只会追加到栈尾，静态资源早已响应完毕，压缩不会生效。
// threshold=1KB：小响应（多数 API JSON）不压缩，省下无谓 CPU。
const app = new App({
    middleware: [
        require('koa-compress')({
            threshold: 1024,
            br: {params: {[require('zlib').constants.BROTLI_PARAM_QUALITY]: 4}},
        }),
    ],
});

// 保留监听句柄：优雅退出时 p2p.shutdown() 第一步就解绑端口，
// 避免 iroh 关闭慢/卡住期间（0~3s 或更久）新进程 bind 不到端口
const listenServer = app.listen(port, async function(err){
    if(err) return;
    Logger.system('MeNote server is ready on http://localhost:' + port);

    // 索引补齐（幂等）：老库不会重跑建表脚本，新增索引在此补上。
    // 失败不影响服务（仅记录警告），故不阻塞后续 P2P 启动
    try {
        const {ensureIndexes} = require('./lib/migrate');
        await ensureIndexes(__dirname);
    } catch(e) {
        Logger.warning('[migrate] 索引补齐跳过: ' + e.message);
    }

    // P2P 服务（lib/p2p.js，官方 @number0/iroh）：随主服务启动
    try {
        const p2p = require('./lib/p2p');
        await p2p.init(app, {listenServer});
    } catch(e) {
        Logger.error('[p2p] 启动失败: ' + e.message);
    }
});
