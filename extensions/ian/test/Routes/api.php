<?php

use Illuminate\Http\Request;

/*
|--------------------------------------------------------------------------
| API Routes
|--------------------------------------------------------------------------
|
| Here is where you can register API routes for your extension. These
| routes are loaded by the RouteServiceProvider within a group which
| is assigned the "api" middleware group. Enjoy building your API!
|
*/

Route::group([
    'prefix' => 'ian/test',
    'middleware'=>'auth:api'
], function () {
    Route::get('/', function () {
        dd('This is the ian/test api page. Build something great!');
    });
});
