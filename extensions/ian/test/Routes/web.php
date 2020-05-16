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
    'prefix' => 'ian/test',
], function () {
    Route::get('/', 'FrontController@index')
        ->name('ian.test');
});
