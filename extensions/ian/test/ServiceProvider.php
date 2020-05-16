<?php
namespace Extensions\Ian\Test;

use Extensions\Ian\Test\Providers\RouteServiceProvider;

class ServiceProvider extends \Illuminate\Support\ServiceProvider
{

    public function boot()
    {
        $this->loadViewsFrom(__DIR__ . '/Resources/Views', 'ian.test');
        $this->loadTranslationsFrom(__DIR__ . '/Resources/Lang', 'ian.test');
        $this->loadMigrationsFrom(__DIR__ . '/Database/Migrations');

    }

    public function register()
    {
        $this->app->register(RouteServiceProvider::class);
    }

}
