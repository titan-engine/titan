const mix = require('laravel-mix');

mix.copy('node_modules/@fortawesome/fontawesome-free/webfonts', 'resources/fonts');

mix.js('resources/js/app.js', 'public/js')
    .sass('resources/sass/font-awesome.scss', 'public/css')
    .sass('resources/sass/app.scss', 'public/css')
    .js('resources/js/admin.js', 'public/js')
    .sass('resources/sass/admin.scss', 'public/css')
    .version();
