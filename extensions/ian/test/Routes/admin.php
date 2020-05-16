<?php

/*
|--------------------------------------------------------------------------
| Web Routes
|--------------------------------------------------------------------------
|
| This file is where you may define all of the routes that are handled
| by your extension. Just tell Laravel the URIs it should respond
| to using a Closure or controller method. Build something great!
|
*/
Route::group([
    'prefix' => 'admin/ian/test',
    'middleware' => ['auth', 'permission:admin', 'update_last_move', 'choose_admin_theme']
], function () {
    Route::get('/', 'AdminController@index')
        ->name('admin.ian.test');
});
