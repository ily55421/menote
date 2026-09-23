const {Controller} = require('jj.js');

class Index extends Controller
{
    async _init() {
        if(!await this.$model.user.is_login()) {
            return this.$redirect('login/index');
        }
    }
    async index() {
        // Vue3 SPA 入口页面
        // 资源版本号取自 package.json：每次发版递增版本号即自动失效浏览器/WebView2
        // 缓存（静态资源 maxage 为 10 天）。此前版本号硬编码在视图里，
        // 发版时容易忘记同步，导致用户端加载到旧 CSS/JS。
        this.$assign('asset_ver', require('../../../package.json').version);
        await this.$fetch();
    }
}

module.exports = Index;
